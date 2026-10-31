# What predictive preloading would take in Quillo, and what prefetching does today

| | |
|---|---|
| App | None: one question, how predictive preloading works and how Quillo could do it, answered with reading, small programs and a study of what browsers offer |
| Quillo | 0.6.0 (401b416), with the uncommitted work of branch `feedback20-fixes`. `src/error.rs` changed after this build; nothing here touches it |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, `curl`, 40 requests at once, Chrome 156 headless, driven by `tests/browser/run.js` and once with no debugger attached. Not run: Firefox or Safari (their path was emulated in Chrome's HTTP cache), `--instances`, `--watch`, Linux, or anything under real traffic |

What worked well: the guard on a prefetch is the strongest part of this, and most frameworks
have nothing like it. A route asked for ahead of a click may read but not write, which is what
makes speculating on a page safe to do by default. Writes refused with a 503 are dropped by the
browser, and the page is made when the link is followed. A page also gets it with nothing
written and nothing set up.

The answer to the question: **predictive preloading is worth adding, but not first.** Today's
prefetching shows stale pages, one of them a signed-in page after logout (Q-176, Q-177). Every
further guess adds to that, so it comes first. Then measurement (Q-181): nobody can see a hit
rate yet, so nobody can tell whether a predictor helps. And the largest gain is not a smarter
guess: links in components get no prefetching at all (Q-180).

Nothing here is fixed: no Quillo source was changed for this report. Fix first, in this order:
Q-176 and Q-177 together, Q-178, Q-179, Q-181, Q-180, and only then a model of what comes next.

## Seen again

None. Q-002 and Q-003, the refused prefetch, are still fixed by their repros: a refused link
loads when followed, and a refusal logs no error.

## How predictive preloading works

Two questions, answered separately: what tells you a visitor is about to go somewhere (the
signal), and how much to get ready (the action).

| Signal | How early | How often right | Quillo |
|---|---|---|---|
| Press or touch | Least: the click is coming | Almost always | Yes |
| Hover for 65 ms, Tab focus | Some | High | Yes |
| Pointer trajectory | About 80 ms sooner, by ForesightJS's own claim | No independent numbers found | No |
| Link in view | Long before | Low, one page render per link | No |
| What people usually do next | At page load | Depends on the site | No |

| Action | Costs | Quillo |
|---|---|---|
| Connect (`preconnect`) | Almost nothing | For libraries, not pages |
| Fetch the document (prefetch) | One page render | Yes |
| Fetch the page's assets | Bytes only, for hashed files | No |
| Fetch the page's data, for a component | One render or call | No |
| Render it, with no scripts (`prerender_until_script`) | Render and assets | No |
| Render it, scripts and all (prerender) | About an iframe, and the page's scripts run | No |

Prefetch fetches only the document, so bundles and CSS still load after the click.

What browsers offer, as far as could be checked:

- **Chrome and Edge:** speculation rules. Eagerness is `immediate`, `eager`, `moderate` (200 ms
  hover) or `conservative` (press). Chrome's 2024 post gives 50 prefetches and 10 prerenders
  for immediate and eager, and 2 of each, first in first out, for the others. A list rule, which
  is what Quillo writes, defaults to `immediate`.
- **Firefox and Safari:** none shipped that could be confirmed. Mozilla's post of May 2026
  prototypes same-origin prefetch behind a preference, and lists WebKit as implementing it.
- **`prerender_until_script`:** an origin trial since Chrome 144, with no commitment to ship.
  It is the type that best suits Quillo, as no script runs before the click. An origin trial is
  not something a language can turn on by default for every program.

Reported gains are weak evidence. Cloudflare gives 45% lower LCP at p75, its own figure for its
own product. Shopify's figures reached us second-hand. Field data by type of rule is
correlational, and says so. No figure here is Quillo's.

## Options

| Option | Verdict | Why |
|---|---|---|
| An author's hint for a known next step (a wizard, the next page) | Do | Very little cost and no guessing |
| Data for links in components | Do, the largest gain | Q-180: reuses what the server already writes |
| Next page from route counts | Do last | Mostly helps touch screens and assets |
| Pointer trajectory | Skip | Little more warning, and more code on every page |
| Every link in view | Skip as a default | One page render per link, most of them wasted |
| Analytics or a trained model (Guess.js) | Skip | Needs an outside data source, and counts should do |
| Prerender | Not by default | Q-178 |
| `prerender_until_script` | Watch | Right idea, not yet part of the platform |

### The design, should it be built

1. **Counts by route, not by address.** Count `from → to` on route patterns (`/products/:id`), in
   memory, as `src/metrics.rs` keeps its counters. No address, no id and no visitor is stored.
   Nothing reads `Referer` today, and it would give the pair for a page load.
2. **Fixed addresses get the document; patterns get the assets.** A model can say "a product
   next", not which of 20. So `/checkout` at p = 0.7 can be prefetched, while `/products/:id`
   warms only its bundle and sheet, and hover fetches the document. Hashed bundles are
   `immutable`, so that cannot go stale and costs once.
3. **It must not learn from its own success.** A click answered from a prefetch never reaches
   the server, so `Referer` under-counts exactly the guesses that were right. The landing page
   has to report them (`deliveryType`, `activationStart`), and a small share of visitors should
   get no prediction, so that the counts and the gain stay honest.
4. **The rule for guessing:** prefetch when `p × the visitor's wait` is more than
   `(1 − p) × the server's cost`. Starting at about p ≥ 0.5 is a guess until there is data.
5. **Links in components:** see Q-180.

## New issues

### Q-176. A page prefetched while signed in is shown after logging out, for 30 seconds

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test in headless Chrome, after reading that Chrome keeps prefetches | 2 |

A prefetch answer gets `cache-control: private, max-age=30` ([server.rs:333](../../src/server.rs#L333)),
for the browsers that have only their HTTP cache to keep it in. But Chrome stores it there too,
and it is stored for the address, with no `Vary: Cookie`. So it outlives the page that asked and
the session it was made for:

```
sign in, point at a link to /me, log out with a form, follow a link to /me
→ {"shows":"ada","delivery":"cache"}
```

The window is narrow: the same browser, the link pointed at first, and 30 seconds. But it fails
without a word, on the page that should be the most private, and it stands against what a
person takes "log out" to mean. The guide mentions this cache, and not that it can do this.

The repro is a folder, as it needs a browser and a form:
[Q-176-a-page-outlives-logout](../repros/Q-176-a-page-outlives-logout).

**Suggestion:** keep `max-age=30` for `x-quillo-prefetch`, the path that needs it, and leave it off
`Sec-Purpose` prefetches, which Chrome keeps itself. Add `Vary: Cookie` to what stays: checked by
hand on a route, the same steps then give `{"shows":"nobody","delivery":""}`. A change that is
not a cookie can still be 30 seconds old on that path, which the guide should say.

### Q-177. A page pointed at and then changed is shown as it was when pointed at

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a test in headless Chrome | 1 |

Chrome keeps a prefetch for the page that asked for it, for about 5 minutes according to MDN. Only
a gap of 1.5 seconds was tested, not the limit. Quillo never tells it that anything changed:

| From the same page | The click shows | From |
|---|---|---|
| Nothing pointed at, then a write | 1, right | the server |
| Pointed at, then a write | **0** | the prefetch |
| Pointed at, then logged in | **"nobody"** | the prefetch |
| Pointed at, then logged out by `fetch` | **"ada"** | the prefetch |
| Firefox and Safari's path, emulated | **0** | the HTTP cache |

This is the case an app with a `server fn` meets: a page stays open while its data changes. Hover
over "Cart", add to the cart, click "Cart", and the old cart shows. Prediction makes it worse, as
pages get fetched sooner and more of them.

A program cannot put this right alone, because of Q-176:

| Sent by hand | The click shows | From |
|---|---|---|
| `Clear-Site-Data: "prefetchCache"` on the write | **0** | the HTTP cache |
| `cache-control: no-cache` on the page | **0** | the prefetch |
| Both | 1, right | the server |

[Q-177-a-page-pointed-at-goes-stale](../repros/Q-177-a-page-pointed-at-goes-stale).

**Suggestion:** send `Clear-Site-Data: "prefetchCache"` (from Chrome 138, by its intent to ship;
tested on 156) on a response that actually wrote. Not on every POST: reads through a `server fn` are POSTs too, and would wipe prefetches for
nothing. Quillo knows what a write is, as the prefetch guard stops on exactly those. Shopify sends
this header when a cart changes. It clears nothing in Firefox or Safari, where only the 30 seconds
limit the damage.

### Q-178. A prerender nobody follows runs the page's components, and their `server fn`s write

| Kind | Detected by | Attempts |
|---|---|---|
| silent | Chrome with no debugger attached | 3 |

The guide says "Prefetching changes nothing", and the book extends that to "speculation rules a
page writes itself". For prerender that holds for the page's `GET` and no further. A prerender runs
the page, the runtime does not know it is one (`src/client/runtime.js` never mentions
`document.prerendering`), and a `server fn` is a POST, which `is_prefetch` does not cover:

```quillo
server fn touch() {
    mounted = mounted + 1
    mounted
}
component Mounts() {
    state done = 0
    <div @mount={(el) => { done = touch() }}>mounted {done}</div>
}
```

With a prerender rule for that page, and nothing clicked, `/counts` says `{"ran":1,"mounted":1}`.
The log has `POST /_quillo/rpc/touch -> 200`. Quillo writes no prerender rules itself, so this
reaches a page that adds one. A browser that starts a prerender on its own, as Chrome can from
the address bar, should meet the same, which was not tested.

[Q-178-a-prerender-runs-server-fns](../repros/Q-178-a-prerender-runs-server-fns).

**Suggestion:** while `document.prerendering`, hold `@mount`, live subscriptions and `server fn`
calls until `prerenderingchange`. Any prerender tier starts here. Until then, the guide should
limit its promise to prefetches.

### Q-179. Prefetches wait in the same line as visitors

| Kind | Detected by | Attempts |
|---|---|---|
| perf | a load test | 2 |

`is_prefetch` is used once, to set the guard. A prefetch takes the same place in line as a visitor.
With one instance and a page of 23 ms:

| In front of the visitor | The visitor's page, three runs |
|---|---|
| Nothing | 0.3, 0.3, 0.4 ms |
| 10 prefetches | 204, 181, 189 ms |
| 40 prefetches | 901, 891, 922 ms |

Forty is many from one visitor, and few from a busy page's visitors all pointing at things. Guesses
are work nobody has asked for yet, and the server gives them what it should give a person.
`--no-prefetch` is the only way out, and it takes a restart. Chrome's guide advises being able
to turn speculation off at short notice, as on a sale day.

[Q-179-prefetches-queue-with-visitors](../repros/Q-179-prefetches-queue-with-visitors).

**Suggestion:** when requests are waiting, answer a prefetch 503 before running it. The browser
drops it, and the click is made as usual. Prediction should not ship without this.

### Q-180. Links in components are not prefetched, and neither is their data

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the runtime, a probe | 1 |

`PREFETCH_JS` leaves out links in components on purpose
([server.rs:1322](../../src/server.rs#L1322)): the runtime follows those, so a document fetched
ahead would be thrown away. Nothing fetches what they would need instead. So a single-page app,
where a link's wait is all data, has the least help of any page: pointing at a link asks for
nothing, and the guide's "show something for `null`" is every navigation.

[Q-180-links-in-components-are-not-prefetched.qwe](../repros/Q-180-links-in-components-are-not-prefetched.qwe).

**Suggestion:** the pieces are there, and this is a design that has not been built. Every path
renders on the server, under the guard, with each component's state in `data-s`, and
`takeHydration` already hands such state to components by name when a page is resumed
([runtime.js:5345](../../src/client/runtime.js#L5345)). So: on hover, fetch the page; keep its
state for a short time; on `navigate`, render from it as if resuming. No new data API, and no
flash of `null`. The risks: it is matched by component name and order, which a differing tree
breaks, and it inherits Q-177, so what is kept must be dropped on a write.

### Q-181. A prefetch looks like a page view, and nothing says how many were used

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the log | 1 |

```
GET / -> 200 (0.1ms)     a prefetch
GET / -> 200 (0.0ms)     a person
```

The log does not tell them apart, and neither does `--log json`, whose two lines differ only in
the time and the request id. `src/metrics.rs` has no prefetch counter. So anything counting the log counts pages nobody saw. Neither is there a hit
rate: how many prefetches were used, and how many were wasted. That matters more with prediction,
as it is the only evidence a predictor helps, and Chrome's guide says the page that asks cannot
see the result: it has to be measured on the page landed on.

[Q-181-a-prefetch-looks-like-a-page-view](../repros/Q-181-a-prefetch-looks-like-a-page-view).

**Suggestion:** mark these lines, add counters for answered and refused, and have a page report
`deliveryType` when it was landed on from a prefetch.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `route GET /mounted = json({ ... })` | Assumed a function, as in other languages | ``cannot call `json`: it is map``: a clear error, but my script did not check that the server started, and waited 180 s on pages that never came | 1 run, 3 minutes |
| `req.query` | Habit from other frameworks | ``unknown name `req` (did you mean `re`?)``: right to refuse, but the guide's `request` was the better suggestion | 1 check |
| Took `{"writes_without_a_click":0}` as prerender being safe | It was the answer I expected | Wrong: a control with a plain script also showed 0. Chrome's `Preload` events gave `PrerenderingDisabledByDevTools`. With no debugger attached, the write happened (Q-178) | 3 attempts; nearly a false "safe" |
| A loop of 1,500,000 additions as a slow page | Assumed a loop is slow | The first run took 117 ms, and then 1 ms, it seems once compiled. Measured so, there was no line at all, and Q-179 would have been missed | 1 run |
| Expected `Clear-Site-Data` to be the whole fix | It is the header made for this | Still stale, from the HTTP cache, which is how Q-176 was understood | 1 run, worth it |
| `set -- $v` in zsh to split a pair | A POSIX shell habit | zsh does not split, and two files were written wrongly | 1 run |

## Limitations

- **Firefox and Safari were not run.** Their path was emulated by making the same request in
  Chrome, so the staleness there is inferred from HTTP caching, which is standard, not seen.
- **The limit of Chrome's prefetch cache was not measured.** "About 5 minutes" is MDN's.
- **Chrome's limits date from 2024.** One later post disagrees about `eager`, and its claim of a
  10 ms hover could not be confirmed.
- **No gain is measured here.** There is no real traffic in this report, so the thresholds are
  starting points, and the claim that Q-180 is the largest gain is a judgement from where the
  waits are, not a measurement.
- **Q-179 is one instance.** With `--instances` the line is shorter, not gone; that was not run.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A click with nothing pointed at, after a write | Fresh: 1 |
| A prefetch used for a click | `deliveryType` is `navigational-prefetch` |
| `Sec-Purpose: prefetch;prerender` | Treated as a prefetch, with `max-age=30`, by the same check |
| A route that sets its own `cache-control` | Kept on a prefetch, `no-cache` and `vary` alike: what the hand tests above rely on |
| `Vary: Cookie` on a prefetch answer, logged out by a form | The right page, made by the server |
| Both fixes by hand, after a write | Fresh: 1 |
| A plain page's answer | No `cache-control` added: only a prefetch gets it |

## Sources

- [Chrome: implementing speculation rules](https://developer.chrome.com/docs/web-platform/implementing-speculation-rules)
- [Chrome: improvements to the Speculation Rules API](https://developer.chrome.com/blog/speculation-rules-improvements)
- [Chrome: prerender until script origin trial](https://developer.chrome.com/blog/prerender-until-script-origin-trial)
- [MDN: Speculation Rules API](https://developer.mozilla.org/en-US/docs/Web/API/Speculation_Rules_API)
- [MDN: `speculationrules`](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/script/type/speculationrules)
- [MDN: Clear-Site-Data](https://developer.mozilla.org/docs/Web/HTTP/Headers/Clear-Site-Data)
- [Blink: intent to ship the Clear-Site-Data values](https://groups.google.com/a/chromium.org/g/blink-dev/c/u3ABOeMzZac)
- [Mozilla: intent to prototype same-origin prefetch](https://groups.google.com/a/mozilla.org/g/dev-platform/c/gBZYcVxrJN4)
- [Shopify: speculation rules](https://shopify.dev/docs/storefronts/themes/best-practices/performance/use-speculation-rules)
- [Cloudflare: Speed Brain](https://developers.cloudflare.com/speed/optimization/content/speed-brain/index.md)
- [Core Web Vitals: speculation rules](https://www.corewebvitals.io/pagespeed/speculation-rules)
- [Core Web Vitals: speculation posture](https://www.corewebvitals.io/state-of-cwv/explorer/metrics.speculation.posture)
- [web.dev: predictive prefetching](https://web.dev/articles/predictive-prefetching?hl=ja)
- [ForesightJS](https://be49cb8b.foresightjs.pages.dev/docs/getting-started/what-is-foresightjs)
