# Feedback from asking how Quillo ships its JavaScript

| | |
|---|---|
| App | None built: questions asked in one session (where a `server fn` runs, what happens when the URL changes, whether users share a bundle, whether a branch's components are in it), each answered from the code and checked by probing: three programs of 10 to 30 lines, the first kept as a repro |
| Quillo | 0.5.0 (cc273cc) |
| Model | Claude Opus 5.5 |
| Tested with | `curl` against `quillo run`: each page's bundle fetched and compared; 1,024 variants of one page, with the server's memory before and after; `feedback/run.mjs` |

What worked well: a page's bundle holds what its components can show and nothing else. An
`App` that `match`es on `url().path` carried both of its branches' components whichever one
was rendered, so moving between them in the browser needs no more JavaScript, and a
component nothing on the page names was left out. Bundles are served
`cache-control: public, max-age=31536000, immutable`, at an address made from their content:
the same program gave the same hash (`05c4ac3c18d2d6b8`) on 72f7913 and cc273cc, so browsers
keep their copy across restarts and releases that do not change it. Nothing about the person
is in a bundle (state and props are in the page's HTML), so everyone who sees the same
components shares one file.

What it cost: how bundles are keyed. The key is the set of components a response rendered,
not what the bundle holds, so the same code is served under two addresses (Q-062); every set
seen is kept in memory until a reload (Q-063); and each bundle carries its own copy of the
runtime, so pages with different components share no JavaScript (Q-064).

## Fixed, on branch feedback7-fixes

Each fix is commented in the code with its issue and the branch.

| ID | The fix |
|---|---|
| Q-062 | A page's bundle is keyed by the components it holds (those rendered and every one they show), and they are compiled in an order that depends only on which they are: `/` and `/products` name the same bundle (src/client.rs, `bundle_for`; the repro) |
| Q-063 | At most 256 sets are kept, the one used longest ago going first; a page naming a bundle no longer kept is sent the whole program's, as after a reload, and the set is made again when a page needs it. `quillo_bundles` under `--metrics` counts them (src/client.rs, `Bundles`; tests/routes.rs, 1,024 variants) |
| Q-064 | The runtime is a file of its own, `/_quillo/r/<hash>.js`: what the whole program's components reach, the same for every page. A page's bundle is its own code, which takes the runtime's names from that file (`window.__quillo_rt`, inside a function, so they do not meet the page's own scripts); a page whose code needs what the file lacks carries its own runtime, as before (src/client.rs, `runtime_file`; server.rs loads both, in order) |

What it changes, measured:

| Program | Before | Now |
|---|---|---|
| The smallest (one component) | one bundle, 31,563 bytes | runtime 31,256 bytes (9,863 gzipped), once; the page's own 428 |
| This report's `App` with `<Home/>` and `<Product/>` | two bundles of 50,200 bytes | runtime 48,927 (14,858 gzipped), once; one bundle of 1,329 for both pages |
| app/pm, four pages | the runtime in each page's bundle | one runtime of 109,886 bytes; the pages' own 27 to 79 KB |

A program's runtime file holds what any of its pages needs, so a program whose pages use
very different parts of the runtime sends a page some it does not use; it is downloaded
once, where the copies were downloaded once per set.

## Seen again

None.

## New issues

### Q-062. Two pages whose bundles hold the same code get two bundles

| Kind | Detected by | Attempts |
|---|---|---|
| perf | comparing the bundles of `/` and `/products` | 1 |

`App` shows `<Home/>` at `/` and `<Product/>` elsewhere. Both are in `App`'s code, so both
are in its bundle either way. The bundles of `/` and `/products` are the same size (50,200
bytes in the probe) and differ in 8 lines, all of them like this:

```
< const $b0 = W.block(["button", [], ["home ", 1], [[2, "click", 0]]])
< const $b1 = W.block(["button", [], ["product ", 1], [[2, "click", 0]]])
---
> const $b0 = W.block(["button", [], ["product ", 1], [[2, "click", 0]]])
> const $b1 = W.block(["button", [], ["home ", 1], [[2, "click", 0]]])
```

The table of bundles is keyed by the components rendered, sorted
([src/interp.rs:1259](../../src/interp.rs#L1259) `bundle_for_page`): `App,Home` and
`App,Product` are two keys. The order their code is compiled in follows the roots, so the
two builds number their blocks differently, hash differently, and are two files. Someone
going from `/` to `/products` by ordinary page loads downloads the same code twice, and
the server keeps it twice.

```quillo
component App() {
    <div>{match url().path {
        "/" => <Home/>
        _ => <Product/>
    }}</div>
}

route GET / = <html><body><App/></body></html>
route GET /products = <html><body><App/></body></html>
```

Repro: [Q-062-same-code-two-bundles.qwe](../repros/Q-062-same-code-two-bundles.qwe)
(open until both pages name the same bundle).

**Suggestion:** drop the roots that another root already reaches before making the key
(`App` reaches `Home` and `Product`, so both pages are `App`), or key by the full set the
bundle will hold; and compile in an order that does not depend on the roots (by name, or
by line).

### Q-063. Every set of components a page has rendered keeps a bundle in memory until the program reloads

| Kind | Detected by | Attempts |
|---|---|---|
| perf | 1,024 variants of one page, the server's memory before and after | 1 |

A page with ten optional widgets (`{on(3) ? <W3/> : null}`), asked for in each of its
1,024 combinations:

| | |
|---|---|
| Distinct bundles | 1,023 (the empty page needs none) |
| One bundle | 35,179 bytes |
| The server's memory (RSS) | 9.4 MB before, 54.9 MB after |
| Time | 0.5 s for all 1,024 |

Each set is built once and kept ([src/interp.rs:1273](../../src/interp.rs#L1273)): nothing
takes one out but a reload (`build_client`). Pages that show components by data (feature
flags, a dashboard's chosen widgets, a role's tools, a CMS page made of blocks) make a set
for each combination people reach, and anyone can ask for combinations nobody uses when the
path or the query picks them.

```sh
python3 - <<'EOF'
import urllib.request, re
seen = set()
for n in range(1024):
    html = urllib.request.urlopen(f"http://127.0.0.1:8080/{format(n, '010b')}").read().decode()
    m = re.search(r"/_quillo/b/([0-9a-f]+)\.js", html)
    if m: seen.add(m.group(1))
print(len(seen), "bundles")
EOF
```

No repro in `repros/`: what the server holds is not in its answers, and `--metrics` does not
count bundles.

**Suggestion:** a bound on the table (least recently used, a few hundred sets), since a
dropped bundle is made again on its next page; past the bound, or for a page with many
roots, the full `/_quillo/quillo.js`, which is one file for every combination. A
`quillo_bundles` gauge under `--metrics` would show it happening.

### Q-064. Each bundle carries its own runtime: pages with different components share no JavaScript

| Kind | Detected by | Attempts |
|---|---|---|
| perf | the sizes of single-widget and ten-widget bundles | 1 |

| Page | Bundle | gzipped |
|---|---|---|
| `<W0/>` alone | 33,584 bytes | 10,560 |
| `<W1/>` alone | 33,584 bytes, another file | 10,560 |
| all ten widgets | 36,459 bytes | 10,863 |

The ten components together add 2.9 KB. The other 33 KB is the runtime (the parts the
components reach), and it is in every bundle. A bundle's address is its content, so `<W0/>`'s
page and `<W1/>`'s share nothing in the browser's cache: someone visiting five pages with
different components downloads the runtime five times, about 10.5 KB each over gzip.

No repro in `repros/`: sizes, not a failure.

**Suggestion:** the runtime as a file of its own, `/_quillo/r/<hash>.js`, cached once for
every page, and the bundles holding only the program's code. Tree-shaking the runtime per
page saves little next to that: the full runtime is downloaded once, while a per-page copy
is downloaded once for every set.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| "The server keeps one bundle per set, so up to 1,024 for ten optional widgets" | Reading `bundle_for_page` | Right (1,023), but said to the user as a fact before it was measured | 1 probe |
| `{(m / 2) % 2 == 1 ? <W1/> : null}` to pick widgets by a number's bits | Integer division, as in most languages | Quillo's `/` on ints gives a fraction unless it divides evenly (the guide says so; Q-044 is its `quillo check` side), so odd masks would show nothing. Seen before running; a string of 0s and 1s instead | 0 runs |
| `grep -c HomeMarker bundle.js` to see which components a bundle holds | Component names in source | Production bundles are minified: no names. Each component's text (`"home "`) showed it | 1 run |

## Limitations

- **No splitting by route.** An `App` that routes to thirty pages ships all thirty in one
  bundle, with what each uses: the price of single-page moves that need no more JavaScript.
  A page that loads code only when its branch is chosen (`lazy <ProductPage/>`) would let
  a large app choose.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| An `App` whose `match` shows `<Home/>` or `<Product/>` | both in its bundle, whichever was rendered |
| A component no page's components name | in no bundle |
| A page of one component that names no other | only that component (33,602 bytes, against 50,200 for the `App`) |
| The same program on 72f7913 and on cc273cc | the same bundle address, `05c4ac3c18d2d6b8` |
| A bundle's headers | `cache-control: public, max-age=31536000, immutable` |
| 1,024 page variants, each building or finding its bundle | 0.5 s for all of them |
