# Feedback from making Quillo's React Quirks videos: findings passed on by a second session, reproduced

| | |
|---|---|
| App | No app of this report's own. Two findings from quillo-videos-36 while it made React Quirks #1 (`~/dev/quillo-videos/videos/178-react-state`, React 19.3 against Quillo), each reproduced for this report in a few lines of its own (the repros below). Three more from the same video went in FEEDBACK80 (Q-1267 to Q-1269), the last of its block. |
| Quillo | `quillo 0.8.6 (238dac2, 2026-11-16)`, the version the videos are pinned to, and `quillo 0.8.8 (6e18cda, 2026-11-20)`, this repository's `target/release/quillo` at the time: both open on both. quillo-videos-36 saw them on 0.8.8 (f35bf2d). Q-1272 was also tried on `quillo 0.8.9 (7801fff, 2026-11-20)`: open. |
| Model | Claude Opus 5.5 (`claude-opus-5-5`), as quillo-videos-37, with the findings from quillo-videos-36 (also Claude Opus 5.5) |
| Found by | quillo-videos-36; reproduced and written up by quillo-videos-37 (Q-1270, Q-1271). Q-1272: quillo-videos-9d (was 37), making Solid 2.0 Quirks #3. Q-1273: quillo-videos-09 (was 95), making video 196 (a Next.js advisory against Quillo); Q-1275: quillo-videos-09, making video 200 (Leptos and Dioxus against Quillo) |
| Tested with | `quillo check`, Google Chrome (headless, through `tests/browser/run.js`), Playwright's Chromium, `feedback/run.mjs` for the repros. |

What worked well: the video put React's first-day surprises next to Quillo's, and most of Quillo's answers held up
when run: two-way `value={x}`, state found by name, `check`'s warning for a dropped `push` in an inline handler.
What follows is where a React developer's habits met a message or a name that didn't help.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| – | none | – |

## New issues

### Q-1270. Lowercase `onchange` on an input runs at each keystroke, as `@input`: the guide maps only React's `onChange` so, and says the DOM's names are compared as written

| Kind | Detected by | Attempts |
|---|---|---|
| silent | quillo-videos-36 (typing "hello" fired it 5 times; and `check` says "`onchange={...}` is `@input`" when both are written), on f35bf2d; reproduced for this report in Chrome and Chromium | 1 |

Q-560's fix made React's `onChange` on an `<input>` or `<textarea>` the DOM's `input` event, and the guide says so:
"Where React's name means another DOM event, it is that one: `onChange` on an `<input>` or a `<textarea>` is
`@input`, each keystroke (the DOM's `change` waits until the field loses focus; `@change` is still that)", and
"The DOM's names are lower case and compared as written". Its first list of events names `onchange` among the plain
HTML ones (`onclick`, `onsubmit`, `oninput`, `onchange`). But lowercase `onchange` is `@input` too: it runs at each
keystroke, and not when the field loses focus, as it does in HTML.

```quillo
component C() {
    state n = 0
    <div><input onchange={() => { n += 1 }}/><p>{n}</p></div>
}
```

Typing "hello" in Chromium, then leaving the field: `n = 5`, then still `n = 5`, on 0.8.6 and 0.8.8 (6e18cda). The
repro, five `input` events and one `change` event in Chrome (`repros/Q-1270-lowercase-onchange-is-input`):
`after 5 input events: n = 5; after a change event: n = 5`. With `@change`, typed into in Chromium and left, it is 0, then 1, on both versions.

| Impact | |
|---|---|
| **Functionality** | A handler meant for "done editing" (save a field, check it once) runs at every keystroke. |
| **User** | A server fn called from `onchange` is called once per letter typed; a plain-HTML reader gets what the guide says is React's meaning. |
| **Requests** | One call per keystroke instead of one per edit, where the handler calls the server. |
| **Security** | None. |

**Suggestion:** keep the mapping for React's camel-case `onChange` only, and let lowercase `onchange` be the DOM's
`change`, as the guide's "compared as written" says. Or, if both are meant to be `@input`, say so beside the first
list of events, and point to `@change` for the HTML meaning.

### Q-1271. `state` inside an `if` in a component is a parse error that doesn't say why

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` (quillo-videos-36: React developers try this, as a hook in an `if`), on f35bf2d; reproduced for this report | 1 |

```quillo
component S(show: bool) {
    if show { state extra = "extra" }
    <p>hi</p>
}
```

`quillo check`: ``error: expected end of statement, found `extra` `` and the note "if the code was written for a
newer Quillo, that would explain it", on 0.8.6 and 0.8.8 (6e18cda) (`repros/Q-1271-state-in-an-if-says-not-why.qwe`).
The message reads `state` as a name and stops at the next word, so it says neither that `state` is only allowed at
the top of a component's body nor where to put it; the note sends the reader looking for a newer Quillo. The guide's
"The rules of hooks have no counterpart: state is found by its name, not by the order of calls, and a statement that
starts something may sit inside an `if`" makes the attempt more likely, not less.

| Impact | |
|---|---|
| **Functionality** | No change: the program is refused, as it should be. |
| **User** | A puzzling message, and a false lead (a newer version), for a mistake React developers make on day one. |
| **Requests** | No change. |
| **Security** | None. |

**Suggestion:** "`state` goes at the top of a component's body, not inside an `if`: declare `state extra = ""` there
and set it in the `if`", without the version note. And in the guide's hooks paragraph, that `state` itself stays at
the top.

### Q-1272. An error value shown in markup prints as `err("...")` on the page, and `check` says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | quillo-videos-9d, making Solid 2.0 Quirks #3 (`~/dev/quillo-videos/videos/192-solid-lists`, `/news-raw`), in Chromium | 1 |

A `server fn` that fails gives an error value, as the guide says ("In a component it is awaited for you and returns an
error value if it fails"). Shown in markup as it is, the value prints its own source form into the page:

```quillo
server fn load_news() -> str ! str = err("503 Service Unavailable")

component News() {
    state news = load_news()
    <p>News: {news}</p>
}
```

The page reads `News: err("503 Service Unavailable")`, on 0.8.6, 0.8.8 (f35bf2d) and 0.8.9 (7801fff), served HTML and in
Chromium alike, and `quillo check` says `main.qwe: ok (1 route, 1 function, 1 component)` on all three
(`repros/Q-1272-error-value-printed-in-markup.qwe`). The page staying up is the good part; the error's text reaching
visitors, in Quillo's own notation, is not, and nothing points at it before it does.

| Impact | |
|---|---|
| **Functionality** | The page works; one spot shows `err("...")` instead of the content or a message. |
| **User** | Visitors see an internal error message, and a programmer's notation, where content should be. |
| **Requests** | No change. |
| **Security** | Low: the error's text (a service's status, a path, a query) can reach visitors. |

**Suggestion:** `quillo check` could warn when a value whose type can be an error (a `server fn` declared `! E`, or a
state set from one) is interpolated in markup without `??`, `is_err` or `?`: "`news` can be an error value here, and it
would show as `err(...)`: give a fallback with `??`, or test `is_err(news)`". Or render an error value in markup as
nothing (or a fixed placeholder) and log it, as an error a statement drops is logged.

### Q-1273. Quillo's security fixes have no published advisories, and nothing tells an app that its pinned version has one

| Kind | Detected by | Attempts |
|---|---|---|
| docs | quillo-videos-09 (was 95), making video 196 ("Your framework had a critical bug. Would you know?", Next.js's middleware advisory against Quillo), 2026-11-21 | 1 |

The video asks how a developer would hear of a security fix. For Next.js, `npm audit` names GHSA-f82v-jwr5-mffw in a
project on 15.2.2, with its severity, from GitHub's advisory database. For Quillo, the fixes are real and serious:
0.8.0's changelog says "One viewer's `live` answer could reach another, for up to two seconds, in 0.7.0 and before:
upgrade", and 0.8.9's Security section lists the fixes of the 0.8.7 security audit. But:

```sh
gh api /repos/quillolang/quillo/security-advisories --jq length    # 0
gh api /repos/quillolang/quillo/contents/CHANGELOG.md --jq .content | base64 -d | grep -c '^### Security'    # 4
```

SECURITY.md says "The fix is released, then the advisory is published", and "Quillo is before 1.0: fixes go into the
latest release only". No advisory has been published for any of them, and the GitHub releases (v0.8.4 to v0.8.9) have
no notes, so the changelog in the repository is the only place a fix is announced. An app pins its Quillo in
`get-quillo.sh` (the videos' apps pin 0.8.6 or 0.8.8), and nothing (not `quillo check`, `quillo version`, nor
get-quillo) says that the pinned version has a known security fix.

| Impact | |
|---|---|
| **Functionality** | None directly. |
| **User** | A visitor of an app on an old Quillo stays exposed to a fixed bug (one viewer's data shown to another, in 0.7.0) for as long as its developer doesn't read the changelog. |
| **Requests** | No change. |
| **Security** | Medium: fixes exist but don't reach the people who need them; the video says so, as Quillo's side of the comparison. |

**Suggestion:** publish a GitHub security advisory for each Security entry from 0.8.0 on (GitHub then shows them to
anyone watching the repository, and they get GHSA ids), as SECURITY.md promises. Put a release's Security lines in its
GitHub release notes. And let `quillo version` or `quillo check` say when a newer release has a security fix, e.g.
from a small signed list on quillolang.org: "quillo 0.7.0: 0.8.0 fixed a security issue (one viewer's live answer
could reach another); upgrade". Opt-out for offline machines.

### Q-1275. A single-page app has no way to set the tab's title: every page keeps the title the server sent, and `<title>` in a component does nothing

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | quillo-videos-09, making video 200 (Leptos 0.8.23 and Dioxus 0.7.10 against Quillo 0.8.8: server rendering, SPA navigation, components, routing), 2026-11-21; quillo-videos-1d saw the same planning its SPA course (spa/COURSE.md, lesson 10) | 1 |

The video's Trail Guide is an SPA as the guide's "Single-page apps" section writes one: `component App()` reads
`url().path`, and `route GET /*rest` renders it for every address. Clicking from `/` to `/trails/2` changes the page
without a load, but the tab still says "Trail Guide" (Chromium, Playwright, 0.8.8 f35bf2d), on every page and in Back's
history list. Leptos's `<Title text=.../>` (leptos_meta) and Dioxus's `document::Title { ... }` set it on the server
and on each client-side navigation, so the video shows this as a gap. What works in Quillo today, about 15 lines
(video 200's `compare/quillo-title/main.qwe`):

```quillo
// in the browser, the page's own <title> element; js() is null on the server, so this does nothing there
fn show_title(text: str) {
    let el = js("document.querySelector", "title")
    if el != null { el.textContent = text }
}
component TrailPage(id: int, tab: str) {
    state trail = load_trail(id)
    if trail != null && !is_err(trail) { show_title(`${trail.name} · Trail Guide`) }
    ...
}
// and the route computes the same title for each address, for the first answer
```

A `<title>` written in a component's markup renders inside the body and is ignored by the browser (quillo-videos-1d
checked this on 0.8.8). Nothing in the guide says how a single-page app should title its pages.

| Impact | |
|---|---|
| **Functionality** | Every page of an SPA has one title: tabs, bookmarks, history and screen readers' page announcements can't tell pages apart. |
| **User** | Back's history list shows the same name five times; a bookmark of `/trails/2` is called "Trail Guide". |
| **Requests** | No change. |
| **Security** | None. |

**Suggestion:** a `title(text)` builtin (or `<title>` honoured in a component) that sets the response's `<title>` while
the page renders on the server and `document.title` in the browser, following state like any other read, the last
one rendered winning, as Leptos's and Dioxus's do. Mention it in "Single-page apps".

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Q-1271's repro marked fixed by `/state/` | the fix would name `state` | "statement" in today's message matched it; the marker is now `` /`state`/ `` | 1 run |

## Limitations

None found beyond the issues.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `@change` on an input, typed into and left (Chromium, 0.8.6 and 0.8.8) | 0 while typing, 1 on leaving the field, as in HTML |
