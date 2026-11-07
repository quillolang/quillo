# Feedback from building a long-read publishing site in Quillo

| | |
|---|---|
| App | quilloer.com: "Quilloer", a Medium-like site in an e-reader's look. Members, stories in Markdown with autosave and drafts, co-writers and editors, writing effort (time, typed vs. pasted) on every story, claps, threaded responses with reactions, highlights in topics, bookmarks and read later, sharing (on the site and to other platforms), referral tracking, newsletters, #hashtags with topic suggestions, an activity grid, badges, invite links with conversion stats, public site stats, SEO (sitemap from the database, JSON-LD, Atom feeds). 29 `.qwe` files, 6,215 lines; 10 migrations, 306 lines of SQL, when the report was written (on 2026-11-04 the app had grown to 36 files, 8,126 lines, and 15 migrations, 418 lines). |
| Quillo | `quillo 0.6.0 (991e96c, 2026-10-29)`, from the starter's `get-quillo.sh` (tag v0.6.0) |
| Model | Claude Opus 5.5 (`claude-opus-5-5`) |
| Tested with | `quillo check` after every change; curl against a copy of the database; headless Chrome driven over the DevTools protocol from Node (typing, clicks, checks that the page did not reload); every inline `<script>` checked with `node --check`; screenshots. Scale: tens of stories, a year of writing sessions; not tested at thousands of rows. |
| Checked | 2026-11-04, by a second session, claim by claim with small probes: on the same release binary (`get-quillo.sh` in a scratch folder, `quillo 0.6.0 (991e96c)`), and on main (`0.7.0-dev`, a9b56bb) to say what is still open. The issues were first numbered Q-038 and Q-039, which FEEDBACK4.md already uses: they are Q-682 and Q-683 now, after FEEDBACK52.md's Q-681. Q-684 was found while checking. |

What worked well: server-rendered pages and forms went a long way. Every page works
without JavaScript, and scripts were added on top for autosave, highlights, suggestions
and in-place follow buttons. Records and route patterns caught bad input before the code
ran. `db.transaction` with `?` rolled back cleanly on every error path tried. `markdown()`
being safe by default meant visitors' text could be rendered without a sanitizer. The
`<script>` escaping of `</` held up: a highlighted passage containing `</script>` went
into a page script safely. `re.replace` with a function (`m.groups`) was enough to add
heading ids and turn #hashtags into links after `markdown()`. `image.info` and
`image.convert` handled uploads (square crop and resize) with no extra library.
`.env`, sessions signed with `QUILLO_SECRET`, `schedule` for the weekly digest, and the
`date` module all worked as documented. A `--watch` reload applied new migrations to a
server that was already running, with no restart.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-029 | `CARD + "where …"` in lib/stories.qwe, where `CARD` is a `` sql`…` `` constant and every value is a parameter: 4 warnings stay in `quillo check` for good. The same shape (`let BASE = sql\`…\`` then `BASE + "where …"`) warns in a 3-line program. Checked: the same warning on main (a9b56bb) | 1 (left as is) |
| Q-008 | `route GET /@:name` (see Q-683) and `\s` in a template string: both errors end with "if the code was written for a newer Quillo, that would explain it", which pointed the wrong way both times. Checked: the note now follows only the lexer's errors (an unknown character, an unknown escape), not the parser's (`1 +* 2` gets none); on main, `\s` still gets it | 2 |

## New issues

### Q-682. `<script>{x}</script>` writes nothing for `null` and `false`, and text written in the braces is not JSON, so the page's script does not parse

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `node --check` on the page's scripts (the browser only logs a syntax error) | 2 |

The guide says "`<script>{x}</script>` writes `x` as JSON". It does for lists, maps,
numbers, `true`, and a variable that holds text. But `null` and `false` write nothing,
whether written in the braces or held in a variable, so `const a = {null};` becomes
`const a = ;`, a syntax error that stops the whole script. `quillo check` reports
nothing. In the app, the story page's highlight script failed for every signed-out
reader, because their default highlight topic was a variable holding `null`.

A text literal written in the braces is a second case: it is written as page text, not
as JSON. `{""}` writes nothing and `{"hello"}` writes `hello`, unquoted, while a
variable holding `""` writes `""`.

```quillo
let none = null
let no = false
let empty = ""
let name = "Ann"
route GET / =
    <html><body>
        <script>const a = {null};</script>         // const a = ;      expected: null
        <script>const g = {none};</script>         // const g = ;      expected: null
        <script>const b = {false};</script>        // const b = ;      expected: false
        <script>const b2 = {no};</script>          // const b2 = ;     expected: false
        <script>const d = {""};</script>           // const d = ;      expected: ""
        <script>const s = {"hello"};</script>      // const s = hello; expected: "hello"
        <script>const d2 = {empty};</script>       // const d2 = "";   correct
        <script>const t = {name};</script>         // const t = "Ann"; correct
        <script>const h = {true};</script>         // const h = true;  correct
        <script>const c = {0};</script>            // const c = 0;     correct
        <script>const e = {[null, false]};</script>// [null,false]     correct
        <script>const f = {{ x: null }};</script>  // {"x":null}       correct
    </body></html>
```

The rule for HTML children (`null` and `false` render nothing) seems to be applied
inside `<script>` before the JSON rule, and a text literal is taken for markup text. A
literal with `<` in it (`{"</script><b>"}`) is written HTML-escaped on 0.6.0
(`&lt;/script&gt;`), and as raw code on main (`<\/script><b>`): neither is JSON, and
neither ends the element early. All of the cases above are the same on main (a9b56bb).

**Workaround:** `<script>{raw(`const a = ${json.stringify(x)};`)}</script>`.

**Suggestion:** inside `<script>`, write every expression as `json.stringify` would,
`null`, `false` and text literals included. Until then, `quillo check` could warn
about an expression in a `<script>` whose type may be `null` or `bool`, and about a
text literal there.

### Q-683. A route path cannot hold `@` (`/@:name`), and the guide does not say what a path may hold

| Kind | Detected by | Attempts |
|---|---|---|
| limitation / docs | `quillo check` | 1 |

Profiles at `/@handle` are common (Medium, Mastodon, YouTube). `route GET /@:name` is
"unexpected character `@`", followed by Q-008's version note. The Routes section
describes `:param`, types and `*rest`, but not which characters a fixed piece may hold.

```quillo
route GET /@:name = name     // main.qwe:1: error: unexpected character `@`
```

Other characters that are valid in a URL path are refused too, mostly with a message
about something else:

| Path | 0.6.0 |
|---|---|
| `/@:name`, `/a@b` | unexpected character `@` (and Q-008's note) |
| `/~:name`, `/a+b`, `/%40:name`, `/a:b` | expected `=` or `{` to start the route body |
| `/a=b` | expected end of statement, found `=` |
| `/x.y`, `/a-b_c`, `/café`, `/caf%C3%A9` | accepted |

On main (a9b56bb), Q-546's fix gives these their own message, ``a path cannot hold `@`
here`` (with a hint for `:`), but `@` and the others are still refused.

**Workaround:** profiles at `/u/:handle`.

**Suggestion:** allow `@`, `~`, `+` and the other characters valid in a URL path in a
piece, as in `/@:name`, or say in the guide which characters a piece may hold.

### Q-684. `image.convert` writes JPEG whatever the target's extension: `out.png` and `out.webp` hold JPEG, and transparency is lost

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the check of this report (`file`, `image.info` on the result) | – |

With no `format` in its options, `image.convert` writes JPEG, even to a file named
`.png` or `.webp`. The guide lists `format: "jpeg"`, `"png"` and `"webp"`, but does not
say that JPEG is the default, or that the extension is not read. A PNG with transparency
converted to `x.png` comes back as `alpha: false` on a solid background, and is served
as `image/png` while holding JPEG. The same on 0.6.0 and on main (a9b56bb).

```quillo
print(image.convert("in.png", "out.png", { width: 20 }))   // { width: 20, height: 13 }
print(image.info("out.png"))      // { width: 20, height: 13, format: "jpeg", alpha: false }
print(image.convert("in.png", "out.webp", { width: 20 }))
print(image.info("out.webp"))     // format: "jpeg"
```

```
$ file out.png out.webp
out.png:  JPEG image data, JFIF standard 1.02, ...
out.webp: JPEG image data, JFIF standard 1.02, ...
```

The app is not affected: its avatars and images are written to `.jpg` with
`format: "jpeg"` (lib/avatar.qwe, lib/images.qwe).

**Suggestion:** take the format from the target's extension when `format` is not given
(`.jpg`/`.jpeg`, `.png`, `.webp`), and give an error value for an extension that names
another format, or none. Say the rule in the guide's Images section.

## Where the AI struggled

Mostly the model's own mistakes, in the order they happened. The language gave a clear
error for most of them; the costly ones were those it did not.

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `old?.body`, `db.one(…)?.id` | JavaScript habit; the guide says there is no `?.` | caught on review before checking; `old.body` works, since a field of `null` is `null` | 3 places |
| `match tab { "a" => 1; "b" => 2 }` | JavaScript/Rust habit | rewritten as `? :` before checking (unsure whether `;` separates arms). Checked: `match tab { "a" => 1; "b" => 2; _ => 3 }` works on 0.6.0 | 1 |
| `\s` in a regular expression inside a `` `…` `` template (a page script) | JavaScript habit | "unknown escape `\s` in string" (with Q-008's note); fixed with `\\s` | 1 check |
| `split("\n")` in page JavaScript inside a template | the `\n` became a real line break inside a JavaScript string | not caught by `quillo check`; found by reading the output; fixed with `\\n` | 1 |
| Profile route `/u/:username`, story route `/s/:id(int)/:slug` declared before `/s/:id(int)/edit` | ordering | `/s/7/edit` matched the story route, which redirected; documented ("the first whose … patterns all match") | 1 curl test |
| `{null}` in a page `<script>` | trusting "writes `x` as JSON" | Q-682 | 2 |
| `__Host-quillo_session` set over plain HTTP from the DevTools protocol | testing | Chrome refused the cookie; documented; tested with `--plain-http` instead | 1 |
| Claimed Quillo printed no warning for a missing `QUILLO_SECRET` | the logs checked never had one | wrong: it prints "QUILLO_SECRET is not set; sessions use a random key and end when the server stops", apparently when sessions are first used. The sign-outs on restart were as documented | told the user, then corrected |
| Deleted `app.db` and stopped whatever listened on the port the user ran on, while testing | carelessness | the user lost an account. Since then every test used `DATABASE=<copy>` (the app opens `env("DATABASE") ?? "app.db"`) on a port of its own, and stopped only its own process | an account |

## Limitations

- **No `@` in route paths** (Q-683): profiles are at `/u/<handle>`.
- **No ids on Markdown headings:** `markdown()` writes `<h2>` with no `id`. For a table of
  contents, the app adds ids with `re.replace` on `str(markdown(body))` and gives the
  result back with `raw`.
- **No text-selection or copy control:** expected. A page script blocks `copy` outside
  code and form fields; anyone can still read the page source.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `markdown("<script>x</script> **b**")` | the HTML is written as text, as documented |
| A highlighted quote containing `</script>`, written into a page script with `raw(json.stringify(…))` | `</` became `<\/`; the script parsed |
| Comments in markup: a `// …` line between elements and inside a list in braces | not written to the page (on 991e96c). Q-031 is listed fixed (2cd134f) in 0.6.0's own feedback/INDEX.md; the "open on 1cf047c" seen first is the public snapshot quillolang/feedback, which stopped at 04538f3 |
| Sessions with `QUILLO_SECRET` in `.env`, across a full stop and start | kept, on a fresh database and an existing one |
| `db.transaction` whose body returns early with `?` on a conflict | rolled back; nothing half-saved |
| `image.convert` with `crop` (square from the middle) and `width: 400` on a PNG upload; a text file as an upload | converted; `image.info` refused the text file before any conversion |
| A `--watch` server with a new migration file added | applied on the next reload, recorded in `_quillo_migrations`. Checked: a new `.sql` file alone does not reload; the next change to a `.qwe` file applies it |
| `re.replace(text, pattern, (m) => …)` | `m.groups` is the list of groups from the first; `$1` also works in a replacement string |
| `schedule("0 8 * * 1", …)` and `schedule("1m", …)` | accepted; not waited for in testing |
