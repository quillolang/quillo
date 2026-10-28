# Feedback from asking what Quillo knows about a program's CSS

| | |
|---|---|
| App | None built: questions asked in one session (can Quillo find unused classes, or duplicate ones; should a component's CSS be scoped), answered by measuring the five apps here (forum, pm, cms, bank, stays) with a tool written for it, [tools/unused-css.mjs](../tools/unused-css.mjs), and two probes kept as repros |
| Quillo | 0.5.0 (f15bead) |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; `unused-css.mjs` on each app, and every class it called unused checked by hand; headless Chrome (`getComputedStyle`) for two components styling one class; the stays home page (`STAYS_DEMO` off, a fresh database) measured as served |

What worked well: markup keeps classes tidy where it can. `class` written twice on one
element is an error (``attribute `class` is written twice in <div>``), and a class written
twice in one (`<div.card.card>`, `.card` with `class="card"`) does no harm: the browser
takes `card card` as `card`. `` css`...` `` passes a sheet through untouched, and the apps' own sheets are nearly all used: of 631 classes the forum, pm, cms
and bank style, 2 are never written (`.avatar.lg` in pm, `.blocks.no-gutter` in cms).

What it cost: Quillo knows nothing of CSS. It cannot say a class is never written (Q-065),
the classes it sets itself are not listed anywhere, so an outside tool that removes unused
CSS removes them (Q-066), a component's CSS is the whole page's (Q-067), and so every page
carries every component's CSS: 83% of the stays home page is `<style>` (Q-068).

## Fixed, on branch feedback8-fixes

Each fix is commented in the code with its issue and the branch.

| ID | The fix |
|---|---|
| Q-067 | A component's or view's CSS is `style css`...`` (or a name) at the start of its body. Each element written there (in helpers written inside it too) carries `data-q-<id>`, made by the parser, so the server's HTML and the browser's are the same and the component is resumed; each rule is made to need it (`.title[data-q-3f9a0c12]`, before any `:hover` or `::before`; inside `@media`, `@supports`, `@container`, `@layer`; `@keyframes` as it is). `:global(...)` keeps a selector, or its start, for the page. A `style` after markup, outside a component or view, or reading the component's parameters is an error (src/parser.rs `style_stmt`, src/styles.rs `scope`). The repro is written with `style` now: the card's title red, the panel's blue, the page's black |
| Q-068 | A page links a sheet of its own in its `<head>`, `/_quillo/s/<hash>.css`, cached for good: the styles of the components and views on it (found by their `data-q-` attributes) and of those its bundle can show; a page loading the whole program's bundle gets every style. At most 256 sheets are kept; one no longer kept is answered with every style (server.rs, src/styles.rs `Styles`) |
| Q-065 | `quillo check` warns of a class a sheet styles (`` css`...` ``, a `style`, a `.css` under `public/`) that nothing can write: "`.never-used` is styled here, but nothing in the program writes it", at the rule's line. Written means text anywhere in the program (match patterns too), its services' text, its `public/` scripts, a template's start (`` `card-${size}` `` for `card-...`), the classes Quillo sets, and the starts a sheet names in `/* set elsewhere: leaflet- */`; not in `:global(...)` (src/check.rs `unused_classes`) |
| Q-066 | The guide lists the classes and attributes Quillo sets, by builtin: `ask`'s and `notify`'s `q-` classes and tones, `data-notices`' places, `highlight`'s `hl-`, `<quillo-media>`'s `data-state`, and `style`'s `data-q-` |

The check on the five apps: forum 0, pm 1 (`.lg`), cms 1 (`.no-gutter`) and bank 0, as
this report found by hand; stays 6, the three Leaflet sets (its kit's MapView could say
`/* set elsewhere: leaflet- */`) and `ui-light`, `ui-dark`, which nothing in stays puts
on a page. The report's other 23 for stays are variants a template makes
(`` `ui-avatar-${size}` ``) that no call asks for; the check counts a template's start as
written, and does not follow which values each call gives.

Stays' home page is unchanged until its kit's CSS is written as `style`: the kit, a
repository of its own, still joins its `` css`...` `` into the page's `<style>`, so 83% of
the page is still that.

## Seen again

None.

## New issues

### Q-067. A component's CSS is the page's: two components that style `.title` restyle each other

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | browser: `getComputedStyle` on three titles | 1 |

A component's CSS is a string the page puts in its `<style>`; the Quillo UI kit keeps each
component's `` css`...` `` next to it (`button_css` in `button.qwe`) and an app joins them
(`<style>{kit_css + stays_css}</style>`). Nothing ties a rule to its component. Two
components that both style `.title`:

```quillo
let card_css = css`.title { color: rgb(200, 0, 0) }`
let panel_css = css`.title { color: rgb(0, 0, 200) }`

component Card() { ... <h2.title #card>card title</h2> ... }
view Panel() = <section.panel><h2.title #panel>panel title</h2></section>

route GET / = <html><head><style>{card_css + panel_css}</style></head><body>
    <Card/> <Panel/> <h2.title #page>the page's own title</h2>
</body></html>
```

| Title | Meant | Shown |
|---|---|---|
| Card's | red | blue: Panel's rule comes later |
| Panel's | blue | blue |
| the page's own | the page's | blue |

`quillo check` says ok, and nothing on the page says why. The kit avoids it by hand: every
class it styles starts `ui-` and then the component's own (`ui-avatar-`, `ui-sd-`), 542 of
them. An app's components get no such help, and two files that both say `.title` find out
in the browser.

Repro: [Q-067-component-css-is-global.qwe](../repros/Q-067-component-css-is-global.qwe)
(card's title blue; red once scoped). It is written the way programs are written today; a fix
that scopes by a new syntax changes how it is written.

**Suggestion:** a style that belongs to a component, `component Card() style card_css { ... }`
(or `style css`...`` in its body), scoped as Vue's `<style scoped>` and Svelte's are: the
compiler gives the component's elements an attribute of its own (`data-q-3f9a`), the same on
the server and in the browser so that resuming still matches the server's HTML, and rewrites
its selectors to need it (`.title[data-q-3f9a]`). `:global(...)` for a rule meant for the
page. With that, Quillo knows each component's CSS, which Q-065 and Q-068 need.

### Q-068. Every page carries every component's CSS, inline: 83% of the stays home page

| Kind | Detected by | Attempts |
|---|---|---|
| perf | the stays home page, as served | 1 |

| | |
|---|---|
| The page | 115,598 bytes |
| Its `<style>` | 96,553 bytes (16,012 gzipped): 83% of the page |
| Classes styled | 542 |
| Of them, on this page | 50 (9%) |

Since nothing ties CSS to a component (Q-067), an app cannot send only the CSS of the
components a page shows: stays sends every sheet of its kit (45 `` css`...` `` blocks) on
every page. And since it is in the page, it is sent again with every page, where a `.css`
file would be cached. (Classes a script adds later, such as the map's, are not in the
server's HTML; the 9% counts the HTML only.)

Quillo already does this for JavaScript: a page's bundle is the code of the components it
rendered, at an address that changes only when the code does (Q-062 to Q-064).

No repro in `repros/`: sizes, not a failure. To measure a page: the `<style>` elements'
length against the page's, and the classes of their selectors against those in the HTML.

**Suggestion:** with component styles (Q-067), a page's sheet made as its bundle is: the
styles of the components it rendered and those they show, at `/_quillo/s/<hash>.css`,
cached for good; the program's own `<style>` stays for what is not a component's.

### Q-065. A class the program styles and never writes is not reported

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `unused-css.mjs` on the five apps, each result checked by hand | 7 (the tool's) |

`quillo check` says nothing of CSS. Measured with
[tools/unused-css.mjs](../tools/unused-css.mjs), which compares the classes of every
selector (in `public/**/*.css` and `` css`...` ``) with every class the `.qwe` files and the
app's own scripts can write:

| App | Styled | Written | Set by Quillo | Maybe (a template's start) | Never written |
|---|---|---|---|---|---|
| forum | 162 | 149 | 13 | 0 | 0 |
| pm | 190 | 171 | 7 | 11 | 1: `.lg` (`.avatar.lg`, a size nothing passes) |
| cms | 173 | 160 | 7 | 5 | 1: `.no-gutter` |
| bank | 106 | 102 | 4 | 0 | 0 |
| stays | 542 | 392 | 0 | 125 | 25 |

Stays' 25 are the kit's, 23 of them variants the app never asks for (`ui-avatar-xs`, `-lg`
and `-xl` where it passes only `size="sm"` and the default; `ui-sd-ok`, `-error`, `-offline`
and `-unknown` where its one `<StatusDot>` is given `"online"` or `"busy"`; `ui-light` and
`ui-dark`, which a page puts on itself to force a theme), and 2 Leaflet's
(`.leaflet-container`, set by Leaflet's script, which no source scan sees).

What makes it hard from outside, each found by the tool getting it wrong first (see
[Where the AI struggled](#where-the-ai-struggled)): classes are made by code
(`` `ui-tabs-${variant}` ``, `` `${unread ? "unseen" : ""}` ``), named in comments, set by
Quillo's runtime (Q-066) and by an app's own scripts (`classList.add("flash")`). The
compiler has all of that: it tells markup from strings from comments, knows the classes its
runtime sets, and sees each view's and component's call sites, so it can tell that
`ui-avatar-${size}` is only ever given `"sm"`.

Repro: [Q-065-unused-class-not-reported.qwe](../repros/Q-065-unused-class-not-reported.qwe)
(`.never-used`, styled and written nowhere: `quillo check` says ok).

**Suggestion:** a warning in `quillo check` (or `quillo check --css`) for a class styled in
a `` css`...` `` or a `public/` sheet that the program cannot write, naming the rule's file
and line: "`.never-used` (main.qwe:10) is styled but nothing writes it". A class a template
may make (`card-${size}`) is not warned about unless no value it is given makes it. With
component styles (Q-067), the check is per component, and exact.

### Q-066. The classes Quillo sets itself are not listed: a tool that removes unused CSS removes them

| Kind | Detected by | Attempts |
|---|---|---|
| docs | `unused-css.mjs` calling 13 of the forum's classes unused | 1 |

`ask` and `notify` make their markup with classes of their own, and `highlight` too:

| Builtin | Classes ([src/client/runtime.js](../../src/client/runtime.js)) |
|---|---|
| `ask`, `notify` | `q-notices`, `q-backdrop`, `q-dialog`, `q-title`, `q-message`, `q-actions`, `q-cancel`, `q-ok`, `q-toasts`, `q-toast`, `q-close`, `q-open`, and `q-` + the tone (`q-danger`, `q-success`) |
| `highlight` | `hl-k`, `hl-s`, `hl-c`, `hl-n`, `hl-f`, `hl-t`, `hl-a` |

The guide says `ask` and `notify` are "markup with `q-` classes ... and styles", and names
none of them; `hl-` is not in it at all. Four of the five apps style the `q-` classes, which
they could only have found in the runtime. No `.qwe` file writes these names, so PurgeCSS
and its kin (and my tool, at first) take them for unused, and removing them leaves every
dialog and toast unstyled, found only when one is opened.

No repro: what the guide leaves out.

**Suggestion:** a table in the guide, by builtin, of the classes and attributes each sets
(the `q-` classes, `hl-`, `data-notices`, `<quillo-media>`'s `data-state`), as the styling
hooks they are; and a stylesheet of them in the examples to start from.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| "Run PurgeCSS over the CSS files" (my first answer to the user) | The usual tool for it | It would have removed the `q-` and `hl-` rules: dialogs, toasts and highlighted code unstyled (Q-066) | caught by measuring |
| The tool split a template on `${...}` and threw the parts away | Only the fixed parts can be classes | Strings inside them are classes too (forum's `` `${unread ? "unseen" : ""}` ``): `unseen` and `first-new` called unused | 1 run |
| Template starts of three characters or more | `r${x}` and `0${n}` made 146 of stays' classes "maybe" | Real starts are one or two (`p${k}`, `s${k % 8 + 1}`, `r-${role}`): 14 of pm's became "unused". A start now counts only where a class can begin | 2 runs |
| Every `"q-..."` class unused | No `.qwe` file names them | Quillo's runtime sets them (Q-066); the tool now reads them from it | 1 run |
| Words in comments counted as written | A string is a string | The kit's usage notes (`<Avatar size="lg"/>`) kept 21 variants nobody uses "maybe" | 1 run |
| `.flash` unused | Only `.qwe` files scanned | cms's own `public/js/cms.js` adds it | 1 run |
| `` css`...` `` rules placed at the line within the block | Lines counted from the block's start | `main.qwe:4` for line 10; now from the file's | 1 run |
| `curl /signin` on stays | The usual path | A 404: stays signs in elsewhere. The home page was enough | 1 request |

## Limitations

- A class a library's script sets (Leaflet's `.leaflet-*`) is in no source the program
  has. A check of unused classes needs a way to say so: a list of starts it should not warn
  about, or a comment on the rule.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `<div class={"a"} class={"b"}>` | ``error: attribute `class` is written twice in <div>`` |
| `<div.card.card>`, `<div.card class="card big">`, `class="card card"` | `class="card card"` and the like: the browser takes it as one |
| `.card` defined twice in one `` css`...` `` | sent as written, the later winning, as CSS does |
| The forum's and bank's sheets | every class written by the program or set by Quillo |
