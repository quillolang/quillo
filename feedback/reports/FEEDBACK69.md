# Feedback from documenting a front end built elsewhere: a React or Vite app's build served beside the program, and the page for a miss that was not sent as written

| | |
|---|---|
| App | None: the guide's new subsection "A front end built elsewhere (React, Vite)", written for the user's question of 2026-11-16 ("is quillo able to render/serve static assets and html including react SPA?"), then "make sure it is documented in the website" |
| Quillo | Checked by hand on the installed `quillo 0.8.4 (8ee269e, 2026-11-15)`. The test of the subsection (`tests/front_end_build.rs`, branch `spa-docs`) was run on this repo's tree at `a57990a` (v0.8.5). The fix (branch `q964-miss-page`) was built from `spa-docs` in a `git archive` copy, as another session held the checkout |
| Model | Claude (Opus 5.5, `claude-opus-5-5`), which wrote the subsection, its test, this report and the fix |
| Tested with | `quillo check` and `quillo run` on a mock Vite build (`dist/index.html`, hashed files under `dist/assets/`, `public/`'s files at the top), curl, and the new test files |

Numbers start at Q-964, after FEEDBACK68.md's Q-963 (number agreed with the session releasing 0.8.5).

What worked well: everything the recipe needs was already there. `files()` with `cache: "1y"` gives the hashed files `immutable`, a last `route GET /*path = file("dist/index.html")` gives every deep link the page with a `200`, a missing chunk stays a `404`, and `quillo check` warns about a route written after the catch-all. The server fn door's rule (`Sec-Fetch-Site` first, then `Origin` against `Host`) explains exactly what Vite's dev proxy does to a call.

## What the guide now says

`USERGUIDE.md`, at the end of "Static files", and a row of "Possible or not":

```quillo
route GET /api/hello = { ok: true }                                    // the program's routes first
route GET /assets/*path = files("dist/assets", path, { cache: "1y" })   // Vite's hashed files
route GET /favicon.ico = file("dist/favicon.ico")                       // each file of public/
route GET /robots.txt = file("dist/robots.txt")
route GET /*path = file("dist/index.html")                              // the app's own paths
```

- `files(..., { not_found: "index.html" })` and `quillo serve dist` both answer a deep link with the page, but with a `404`. That is why the catch-all is `file()`.
- The app calls a `server fn` with `POST /_quillo/rpc/<name>`, only from the program's own origin. Another host, port or subdomain of the same site gets a `403`.
- Vite's short proxy form (`"/api": "http://localhost:8080"`) sets `changeOrigin: true`. A browser still gets through, as it says `Sec-Fetch-Site: same-origin`. A client that sends only `Origin` gets a `403`. `changeOrigin: false` works for both.
- React's server rendering is not Quillo's: `js()` is null on the server.

Correction to an earlier note (the mn and React study, 2026-11-08): "Vite's short proxy form gets 403" holds only for a client that sends no `Sec-Fetch-Site`.

## New issues

### Q-964. The page `files()` sends for a miss has the prefetching script added, and `--watch`'s, against "Pages sent as written"

| Kind | Detected by | Attempts |
|---|---|---|
| quiet (the page works, its bytes are not the file's) | `tests/front_end_build.rs`, asserting the `not_found` page's body | 1 |

```quillo
route GET /*path = files("dist", path, { not_found: "index.html" })
```

With `dist/index.html` holding a `<script>` (any built app's page does), `GET /products/42` on 0.8.5:

```
HTTP/1.1 404 Not Found
content-type: text/html; charset=utf-8
cache-control: no-cache

<!doctype html>…<div id="root"></div><script>(()=>{const d=document,k=Symbol.for("quillo")…w.pf=1…})()</script></body></html>
```

The same file through `file("dist/index.html")` goes out as it is on disk. A folder's `404.html` with a link gets the same script, and under `--watch` the dev script too.

The guide says: "**Pages sent as written:** a file `file()` or `files()` serves, and any answer with an `etag` or a range, goes out as its bytes are … no prefetching script is added to it, nor `--watch`'s." The server's test for that is `as_written` (src/server.rs): status 206, an `ETag` or a `Content-Range`. The miss path (src/builtins.rs, `files`) takes the page's `ETag` off, rightly, since it is another URL's validator (FEEDBACK55.md, Q-715). So the page no longer looks like a file, and gets what a route's page gets.

Expected: the page for a miss is sent as its bytes are, as the guide says, without its `ETag`.

| Impact | |
|---|---|
| **Functionality** | A miss's page gets an inline script of about 1.5 KB that its file does not have. A page with no link, script or component is not touched |
| **User** | Hovering a link on a 404 page prefetches it. Under `--watch` the 404 page reloads at a save. Neither breaks anything; both are what the guide says does not happen |
| **Requests** | About 1.5 KB more on every miss with a link or script, and the prefetches started from those pages |
| **Security** | No data exposed. A site with a strict CSP (`script-src` without `'unsafe-inline'`) gets a CSP violation on each such 404 page |

**Fix (branch `q964-miss-page`):** the miss path marks its answer with an internal header (`x-quillo-as-written`). `as_written` honours it only on a 404, so a page a route made pays one integer comparison. `answer` takes the mark off every 404 once both scripts are decided, so it is never sent, through the HTTP server, the phone embed or a mounted program. A program that writes the mark itself on a 404 gets what an `etag` it wrote would give: the page sent as it made it. On any other status the mark means nothing to Quillo and is sent as the program's header. Tests: `tests/feedback69.rs` (4 tests: the page as written for `404.html`, a named `not_found`, `HEAD`, a range and conditionals, a text page, a mounted program and `--watch`; a 404 a route makes still prefetches; the mark forged), and `tests/front_end_build.rs` now asserts the page exactly.

**Found in review (a review agent on 24ccd9f, read-only):** `reply 200 files(...)` (and any `reply` with a status other than 404) kept every header, the mark included, and the strip in `answer` runs on 404s only, so the mark went out on the wire; a program could also read it (`files(...).headers`) and copy it into a page of its own, which then lost its scripts and its components' bundle. Fixed on the same branch: a `reply` with another status drops the mark (that page is the program's, and gets what a route's page gets, as in 0.8.5), `respond`, `stream`, `events` and `site_headers` refuse a header of that name ("is Quillo's own"), and a program reading an answer's headers, or the debugger, does not see it. So only `files()` sets it, only a 404 carries it, and `answer` takes it off. Tests: `a_miss_given_another_status_is_the_programs_answer`, `a_program_reading_the_answer_does_not_see_the_mark`, and `the_mark_is_quillos_and_no_program_sets_it` in place of the test that let a program set it; the mounted program's page is now told apart from the main one's and asserted exactly.

Measured with `cyc.py` (cycles a request for the whole process, `/usr/bin/time -l`; medians of 6 rounds of 5 s, 64 connections), base = `spa-docs` d94b972, new = the fix, both release builds from one target dir:

| Scenario | Base cycles/req | New cycles/req | Base instr/req | New instr/req |
|---|---|---|---|---|
| Fortunes (a page a route makes) | 23,016 | 23,032 (+0.07%, noise) | 75,926 | 75,863 |
| Text | 16,556 | 16,486 (−0.4%) | 36,520 | 36,440 |
| `files()` miss with a `404.html` | 47,377 | 46,259 (−2.4%) | 180,886 | 177,512 (−1.9%) |

The tests fail on the base binary (3 of `feedback69`'s 4, and `front_end_build`'s `not_found` test) and pass on the fix. The test files that cover `files()`, misses, `ETag`s and prefetching pass on the fix: 715 tests in 13 files (feedback20, 36, 55, 69, cover_builtins, cover_feedback55, front_end_build, handle_edges, prefetching, routes, serve, server_parts, urls).
