# Feedback from building thirty YouTube Shorts about Quillo

| | |
|---|---|
| App | None in `app/`: thirty vertical videos about Quillo (YouTube Shorts 31 to 60), kept in `quillo-videos/shorts/`. 21 have an app: pages, routes driven with curl, and scripts, each 3 to 41 lines (405 in all). The other 9 are REPL sessions. |
| Quillo | Built with the released `quillo 0.8.6 (238dac2, 2026-11-16)`. Each new issue below was checked again on this checkout's `quillo 0.8.6 (66468c1, 2026-11-17)` (branch `security`). |
| Model | Claude (Opus 5.5, `claude-opus-5-5`). Five agents built six Shorts each from the short guide (`v0.8.6/llms.txt`). One session reviewed every video and checked each finding against the full guide (`v0.8.6/llms-full.txt`). |
| Tested with | `quillo check`, `quillo run`, `quillo repl` (under `script`, so the terminal text is real); headless Chromium driven by Playwright (each Short's screenshots, with the page read back); curl on port 8080 |

Issue numbers Q-1020 to Q-1026 (Q-1025 and Q-1026 were added later the same day, from the Shorts 63-just-html and
62-vs-react-setup). The block Q-1020 to Q-1029 was agreed with the sessions that write FEEDBACK71, FEEDBACK72 and the security batch. This is a report only: no code, INDEX.md or `repros/` changed. The repros are written out below.

What worked well: every topic planned for these Shorts existed in 0.8.6 and did what its line in the guide says. All thirty show real runs, and no program needed a workaround for Quillo itself. Where a first draft was wrong, the message usually named the fix:
- `server fn join` hides a builtin;
- `live(search, [q])` failed in the page with "argument `q` of `search` must be str, got list";
- `quillo run greet.qwe Ada` answered "a program's own arguments go after `--`, as `quillo run tool.qwe -- Ada`".

Measured on the way:
- `defer(load())` sent the page in 126 ms and the rows at 2115 ms, in the same answer (`load` sleeps 2 s).
- `run_after(2000, ...)` hid its message after 2.01 s.
- `crypto.sha256` and `crypto.hmac` matched `shasum` and `openssl` byte for byte.

## Seen again

None of INDEX.md's open issues came up. Two known behaviours were met, and both worked as designed:
- **Q-544.** A pattern miss answers a bare 400, and the log has the closest route and why: "`query.page` is text, not an int". 39-query-pattern shows that log line.
- **FEEDBACK72's note.** A `{ json: User }` route needs `content-type: application/json`, so 58-json-body used `curl --json` from the start.

## New issues

### Q-1020. An event handler in a view placed in a page is dropped, and `quillo check` says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (Playwright) | 1 |

The guide says a view is made on the server only, and adds nothing to the browser bundle unless a component uses it. A view placed directly in a page therefore never runs its `@click`. The server sends a bare `<button>`, and clicking it does nothing. `quillo check` passes it without a word, though it can see both the handler and where the view is placed.

The same view inside a component does work in the browser, because it is in that component's bundle. So the same lines behave differently depending on where the view is used.

```quillo
// A view with a click handler, and a component with the same one.
state likes = 0

view LikeView() =
    <button @click={() => { likes += 1 }}>{likes}</button>

component LikeButton() {
    <button @click={() => { likes += 1 }}>{likes}</button>
}

route GET / = <html><body><LikeView/><LikeButton/></body></html>
```

What happens:
- `quillo check`: `main.qwe: ok (1 route, 1 function, 1 component)`.
- `GET /` sends `<body><button>0</button><quillo-island style="display:contents" data-c="LikeButton" ...><button>0</button></quillo-island>`.
- Clicked three times each, the first button stays at 0 and the second counts to 3.

49-views ("View or component? Same button, only one works") shows this on purpose. The difference came up when checking whether the video could say that "a view never runs in the browser". It can't: only that a view placed in a page doesn't.

| Impact | |
|---|---|
| **Functionality** | A handler that never runs: whatever it would change stays as it was |
| **User** | A button that does nothing, with nothing in the console or the log |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** have `quillo check` warn of an event attribute (`onclick`, `@click` and the rest) in a view that a page places outside any component, for example: "`@click` in view LikeView never runs: LikeView is placed in a page (main.qwe:11), not in a component. Make it a component, or place it inside one."

### Q-1021. `date.format` with a locale it has no data for gives English without a warning; `format_number` and `money.format` warn

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run | 1 |

The full guide lists `date.format`'s locales (English, German, French, Spanish, Italian, Dutch, Portuguese, and the regions CLDR gives their own) and says "any other language gives English". That is what happens, but silently:
- `ja-JP`, `zh-CN`, `ko-KR`, `ar-EG`, `hi-IN` and `ru-RU` all give "Tuesday, November 17, 2026";
- so does `xx-YY`, which is no locale at all.

`format_number` and `money.format` print a warning in the same case.

```quillo
let d = date.parse("2026-11-17T09:10:06+09:00")
print(date.format(d, { locale: "ja-JP", style: "full" }))   // Tuesday, November 17, 2026
print(date.format(d, { locale: "xx-YY", style: "full" }))   // Tuesday, November 17, 2026
print(format_number(1.5, "xx-YY", 1))                       // 1.5
// quillo: warning: no money and number data for locale "xx-YY": written as en-US
//   (for format_number only; nothing for either date.format)
```

48-time-zones tried to show Tokyo's date in Japanese. The English output looked like a bug in the program until the list in the full guide was found. The short guide's line (`{ locale, style }`) names no languages.

| Impact | |
|---|---|
| **Functionality** | Dates in English on a page translated into a language Quillo has no date names for, or with a mistyped locale |
| **User** | A visitor reading Japanese sees an English date among translated text; the developer gets no hint why |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** give the same warning as `format_number`, for example `no date data for locale "ja-JP": written in English`. Also name the languages on the short guide's `date.format` line.

### Q-1025. A lowercase `onchange` on a text input runs on every keystroke, as React's `onChange` does, not when the field is left as HTML's does

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (Playwright) | 1 |

The guide lists `onchange` among plain event attributes ("Events are `onclick`, `onsubmit`, `oninput`, `onchange`"). It maps only React's camelCase name to another event: "`onChange` on an `<input>` or a `<textarea>` is `@input`". In 0.8.6 the lowercase HTML spelling is mapped the same way. So an `onchange` written as HTML means it (once, when the field is committed) runs on every key, and only `@change` keeps the DOM's `change`.

```quillo
// onchange, @change and oninput, side by side.
component Box() {
    state a = 0
    state b = 0
    state c = 0
    <section>
        <input id="a" onchange={() => { a += 1 }}/>
        <input id="b" @change={() => { b += 1 }}/>
        <input id="c" oninput={() => { c += 1 }}/>
        <p id="out">{a} {b} {c}</p>
    </section>
}
route GET / = <html><body><Box/></body></html>
```

In Chromium, typing "xyz" into each box and then blurring it gave the same result on the released 0.8.6 (238dac2) and on this checkout's c616da3:

| Box | After 3 keys | After blur | HTML's own `onchange` would give |
|---|---|---|---|
| `onchange` | 3 | 3 | 0, then 1 |
| `@change` | 0 | 1 | 0, then 1 |
| `oninput` | 3 | 3 | 3, then 3 |

It came up while making the Short "It's just HTML" (63-just-html): a test of React's `onChange` showed lowercase `onchange` behaving the same.

| Impact | |
|---|---|
| **Functionality** | Work meant for a finished value (save the field, validate it, call the server) runs on every keystroke |
| **User** | Validation messages while typing, and a `server fn` call per key; someone writing plain HTML gets no hint why |
| **Requests** | One call per keystroke where one per field was meant, for a handler that calls the server |
| **Security** | No change |

**Suggestion:** map only the camelCase `onChange` (React's name) to `input`, and keep lowercase `onchange` as HTML's `change`, as `@change` already is. If the mapping is meant for both spellings, say so on the guide's events line.

### Q-1022. `quillo check` warns of classes that the HTML under `public/` writes

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

Q-065's check counts a class as written when it appears in:
- text anywhere in the program;
- its services' text;
- a script under `public/`.

HTML files under `public/` are not counted. A static site served by `files("public", path)`, or by `quillo serve`, keeps its classes in those pages, so each one is reported. This is related to Q-1012 (a script beside an app reads the app's sheets), but here it is the program that serves them.

```sh
# main.qwe:           route GET /*path = files("public", path)
# public/index.html:  <link rel="stylesheet" href="style.css"><h1 class="hero">Hi</h1>
# public/style.css:   .hero { color: tomato; }
#                     .card { padding: 1em; }
quillo check
# ./public/style.css:1: warning: `.hero` is styled here, but nothing in the program writes it
# ./public/style.css:2: warning: `.card` is styled here, but nothing in the program writes it
# main.qwe: ok (1 route, 0 functions, 0 components), 2 warnings
```

The `.card` warning is right: nothing writes it. The `.hero` warning is wrong. To get a clean check, 59-static-files gave its pages `data-page` attributes in place of classes. The guide's `/* set elsewhere: hero */` would also work, one class or prefix at a time.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | Every class of a static site is reported, so a real warning gets lost among them, or the developer bends the HTML to silence them |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** count the classes in `public/**/*.html` as written, as `public/` scripts already are, and do the same for any folder that a `files()` call serves.

### Q-1026. A page's runtime is about 3.3 times the size the guide gives, and is not minified

| Kind | Detected by | Attempts |
|---|---|---|
| docs (or perf) | measuring the bytes a page loads (Playwright and curl) | 1 |

The guide says each page loads "two scripts, minified", with the runtime "about 35 KB, 11 KB gzipped, for a page with one
counter". Elsewhere it says "9 KB compressed for a simple component". For React's own counter written in Quillo, the
runtime script is 132,544 B raw and 36,498 B with `gzip -9`. Quillo's own server sends it as 36,640 B.

It is not minified either: 644 lines, with 1,282 ` = ` written with spaces around them, and readable names. It is
trimmed to the program (30-life's runtime is 147 KB, 12-todo's 145 KB), as the guide says.

```quillo
// The counter from React's docs, in Quillo.

component Counter() {
    state count = 0
    <button onclick={() => { count += 1 }}>
        Clicked {count} times
    </button>
}

route GET / = <html><body><Counter/></body></html>
```

```sh
quillo run main.qwe          # not --watch
curl -s localhost:8080/ | grep -o '/_quillo/[rb]/[a-z0-9]*\.js'
# /_quillo/b/9b45f4ff42fb881f.js    707 B raw,     428 B gzip -9
# /_quillo/r/7f105a7ac125efd0.js 132544 B raw,  36498 B gzip -9, 644 lines
```

The released 0.8.6 (238dac2) and this checkout's c616da3 give the same, within 140 bytes. It came up in
62-vs-react-setup, which counts the JavaScript each side downloads until the first click. With the inline scripts,
Quillo's page loads 38,762 B against React's 67,769 B for Vite's production build (React 19.3.1). Quillo is ahead
by 1.7 times, where the guide's numbers suggest about 6.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A first visit downloads about 25 KB more (gzipped) than the guide leads one to expect; anyone who compares Quillo with another framework from the guide's figure overstates it |
| **Requests** | No change in their number |
| **Security** | No change |

**Suggestion:** minify the runtime in production (whitespace and local names), or, if this is the intended size, give
the measured figure in the guide (about 130 KB, 36 KB gzipped, for one counter) and drop "minified".

### Q-1023. The REPL prints nothing for an expression whose value is null

| Kind | Detected by | Attempts |
|---|---|---|
| docs | `quillo repl` | 1 |

The guide's section on the REPL says it "prints each expression's value". A null prints nothing, so a missing field looks like a line that did nothing:

```
quillo> let p = { name: "Rex" }
quillo> p.age
quillo> p.name
"Rex"
```

45-json-parse wanted to show that a missing field is null, and showed `pet.age == null` and its `true` instead. The guide's own example relies on the silence (`db.open("app.db")` prints nothing), so printing `null` after every call would add noise.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | Someone checking whether a field exists gets no answer, and can't tell a null from a statement |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** say it in the guide: "a null is not printed, as in Python's REPL; `x == null` shows it". Or print `null`, dimmed, for a bare expression that is not a call.

### Q-1024. The short guide (`llms.txt`) leaves out `while`, `live`'s arguments, that `ask` and `notify` are unstyled, and `quillo serve`

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading the guide, then a review against `llms-full.txt` | 5 |

The agents were given `v0.8.6/llms.txt` (52 KB) as their guide. It links the full manual (`llms-full.txt`, 917 KB) once, at the end. Four things missing from the short guide led to detours. They also led the reviewing session to report bugs that were not bugs, all withdrawn once it read the full guide.

| Short guide | Full guide | What the model did |
|---|---|---|
| no `while` (its 8 matches are prose, like "while the server renders") | `while cond { ... }`, with what it gives | 36-guessing-game loops by recursion, `play(tries + 1)`: "the guide has no while" |
| `live(load, args)` | `live(load, ...args)`, `live(load, id)` | the plan wrote `live(search, [q])`, and the page failed with "argument `q` of `search` must be str, got list" |
| `ask(message, options)`, `notify(message, options)`: "Asks in a dialog on the page", "Shows a note on the page" | markup with `q-` classes that the page lays out and styles, and the options `title`, `ok`, `cancel`, `tone` and `seconds` | 17-notify and 40-ask each shipped a CSS file, which was right, but the unstyled dialog was reported as a bug and the options as undocumented |
| no `quillo serve` | `quillo serve <folder>` under Static files, with 16 mentions | reported as missing from the guide |

The brief chose the short guide, and that choice cost time. But it is also the file a model finds first when it looks for the docs written for it.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | Code written around features that exist (recursion for a loop), CSS written without the class names, and false bug reports |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** in `llms.txt`:
- add a `while` line next to `for`;
- write `live(load, ...args)`, with `live(search, q)` as the example;
- say in one sentence that `ask` and `notify` are plain markup with `q-` classes for the page to style, with the class names in the full manual;
- list `quillo serve <folder>` among the commands;
- move the link to `llms-full.txt` near the top, saying what is only there.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `live(search, [q])` | read `live(load, args)` as a list | the page failed, naming the argument and its type | 1 run |
| a recursive `play(tries + 1)` for the guessing game | no `while` in the short guide | works, and was kept (Q-1024) | none |
| `server fn join(name)` | the natural name | `quillo check`: it hides a builtin | 1 check; renamed `signup` |
| `input { width: 1em }` to enlarge checkboxes | inputs don't take the page's font size | tiny boxes, so `zoom: 2` instead | 1 still; the browser's doing, not Quillo's |
| a bare `json.stringify(pet, true)` in the REPL | expected the indented text | the REPL shows the string's value, escaped, on one line | 1; `print(...)` |
| `date.format(d, { locale: "ja-JP", style: "full" })` | wanted the date in Japanese | English, without a word (Q-1021) | 1 |
| classes in `public/style.css` for a static site | ordinary CSS | false warnings (Q-1022) | 1; `data-page` attributes |
| bug reports for `while`, `quillo serve`, the unstyled `ask` and `notify`, `live`'s arguments and `date.format`'s locales | the reviewer read only the short guide | all of them are in the full guide; withdrawn | one review |

## Limitations

None that cost a Short anything.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `route GET /todos/:id(int)` asked for `/todos/abc` | no route matches: 404 Not Found |
| `route GET /search ? { q: str, page: int = 1 }` | the default 1 fills in; `page=3` arrives as an int; `page=abc` is a 400, and the log names `query.page` |
| `{ json: User }` given an extra `"admin": true` | 201, and the stored user has only `name` and `age` |
| `{ json: User }` with `age` missing | 400, "bad request: no route pattern matches this request" |
| `set_cookie("name", name)` | `set-cookie: name=Ada; Path=/; HttpOnly; Secure; SameSite=Lax` |
| `respond(404, "no such todo")`, `redirect("/")` | the 404 with its body; 303 See Other with `location: /` |
| `files("public", path)` | `/` gives index.html, `/about` about.html, and `/nope` 404.html with status 404 |
| `db.error_kind(r)` | "unique" for a UNIQUE constraint, "not_null" for NOT NULL |
| `db.migrate` after adding `002_*.sql` | only 002 runs; the old row keeps its data and gets the new column's default |
| `defer(load())` | the page at 126 ms, the rows at 2115 ms, in one answer |
| `run_after(2000, f)` | ran 2.01 s after the click |
| `ask` in a handler | an in-page dialog with `role="alertdialog"`: Cancel gives false and OK true; no native dialog opens |
| `publish("news")` with `watch("news")` in three tabs | both watching pages updated; with the `publish` line removed, neither did |
| `channel.open` and `channel.broadcast` between two tabs | eight messages back and forth, with no database |
| `live(search, q)` and `waiting(found)` | runs again as `q` changes; `waiting` is true on a connection with 3 s latency, while the old list stays |
| `quillo run greet.qwe Ada` | "unknown argument `Ada`: a program's own arguments go after `--`, as `quillo run tool.qwe -- Ada`" |
| `env("API_KEY", "none")` | `.env` is found from any working folder, the shell's variable wins, and the default applies without the file |
| `crypto.sha256`, `crypto.hmac` | match `shasum -a 256` and `openssl dgst -hmac` |
| `url_encode({ q: "café & co", page: 2, sort: null })` | `q=caf%C3%A9%20%26%20co&page=2` |
| `format_number(1234567.891, locale, 2)` | en-US 1,234,567.89; de-DE 1.234.567,89; fr-FR with narrow no-break spaces (U+202F); en-IN 12,34,567.89 |
| `stats.describe([87, 70, 93, 79, 71])` | count 5, mean 80.0, std 10.0 (the sample's), median 79.0, q1 71.0, q3 87.0 |
| `fn add_one(s) = int(s)? + 1` in the REPL | `add_one("41")` is 42, and `add_one("pizza")` gives the error back |
| a view used inside a component | its handlers work (compare Q-1020) |
