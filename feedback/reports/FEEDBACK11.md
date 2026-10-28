# Feedback from asking whether a Quillo program can use Tailwind

| | |
|---|---|
| App | None built: one question asked in a session ("if someone wants to use Tailwind, there is no way for them to inject a CSS process, right?"), answered from the guide and by a probe of `--watch` with a generated stylesheet. Tailwind's own CLI could not be run (see [Where the AI struggled](#where-the-ai-struggled)) |
| Quillo | 0.5.0 (af64ac8) |
| Model | Claude Opus 5.5 |
| Tested with | `quillo run --watch`, headless Chrome (`feedback/tools/drive.mjs`): a page's computed style and a marker on `window`, read before and after the stylesheet was rewritten (as `tailwindcss --watch` rewrites it), then after the `.qwe` file was touched |

What worked well: a stylesheet made by another tool is served like any file
(`route GET /assets/*path = file("public/" + path)`), with an `ETag` and `no-cache`, so a page
loaded after the sheet changes gets the new one; and touching a `.qwe` file under `--watch`
reloads the program and refreshes the open page (`quillo: reloaded main.qwe (2 routes)`),
which then had the new rules. Classes written as strings (`class="px-3 py-1.5 bg-blue-600"`,
`n > 3 ? "text-red-600" : "text-gray-700"`) are the kind Tailwind's scanner reads from any
file, `.qwe` included.

What it cost: Quillo has no step where CSS is made. Tailwind (or PostCSS, Sass, Lightning CSS)
runs beside it as a process of its own, started by hand, and `--watch` does not see what it
writes (Q-078); nor can a component's own `style` be processed (Q-079).

## Fixed, on branch feedback11-fixes

Each fix is commented in the code with its issue and the branch.

| ID | The fix |
|---|---|
| Q-078 | Under `--watch`, the files the program served with `file()` are watched with its own. A stylesheet that changes is swapped in the open pages: each `<link>` to its path is replaced by one asked for afresh (`?quillo=<time>`), the old one taken out once the new one has loaded, on the `--watch` connection (`css:N:["/assets/app.css"]`), so the page keeps its state; any other file served refreshes them. The report's probe again, the sheet rewritten as Tailwind's `--watch` writes it: `font-weight` 400 → 700 in under 1.5 s, `window.__mark` kept, a component's count (2) kept, one `<link>`; ``quillo: /assets/app.css changed: swapped in open pages`` in the log (src/server.rs `watch`, `dev_script`, `serve_events`; src/statics.rs `served`) |
| Q-079 | The suggestion's first two steps. `quillo run main.qwe --watch --with "npx tailwindcss -i src/app.css -o public/app.css --watch"`: a command run beside the program, in its folder, in a process group of its own, its lines marked with its name (`[tailwindcss] ...`), stopped with the program, group and all; more than one `--with` for several. And `style_with("npx postcss")` at the top level: every component's and view's `style` goes through the command (CSS in, CSS out) as the program loads, before it is scoped, so `@apply` and `theme()` work in a component's own CSS with PostCSS and Tailwind's plugin. All the styles go through at once, each after a `/*! quillo-style <id> */` mark such tools keep (each on its own when the marks do not come back); what it made is kept while the CSS is the same; a command that fails stops the program loading with what it said (src/main.rs `with`, src/styles.rs `made_with`; a unit test with `sed` standing in for PostCSS) |

The shorthand question, answered afterwards with Tailwind 3.4.17 (`npx tailwindcss
--content "./*.qwe"`) on a program written with it: Tailwind's scanner finds the
shorthand's classes (`<div.flex.p-4.gap-2.items-center.bg-blue-600>`: all five), those in
`class="..."`, both sides of a `? :`, and the variants `sm:p-2` and `md:flex-row`; not
`` `border-${tone}-500` ``, made from parts, as expected; and not `w-1/3` written right
before `/>` (`<i.w-1/3/>`, read as `w-1/3/`): a space before `/>` is enough. Running it
showed a silent bug, fixed on branch shorthand-colon: `<em.hover:underline>` was
`<em class="hover" :underline>`, the `:` taken as the start of an attribute, so Tailwind's
`.hover\:underline:hover` styled nothing. The shorthand now takes `:` before a letter or a
digit and `/` before a digit: `class="hover:underline"`, `class="md:flex w-1/2"`.

Not done: the third step, utility classes made by Quillo itself, which the report leaves as
a decision for later.

## Seen again

None.

## New issues

### Q-078. `--watch` does not refresh a page when a stylesheet it serves changes

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | browser: computed style and a marker on `window`, the sheet rewritten under `--watch` | 1 |

The way to use Tailwind today is its CLI beside Quillo:

```sh
npx tailwindcss -i src/app.css -o public/app.css --content "./**/*.qwe" --watch &
quillo run main.qwe --watch
```

```quillo
route GET /assets/*path = file("public/" + path)
route GET / = <html><head><link rel="stylesheet" href="/assets/app.css"/></head><body>...</body></html>
```

`public/app.css` rewritten under a page that shows `<span class="text-red-600 font-bold">`, a
`.font-bold` rule added:

| | `window.__mark` | `font-weight` |
|---|---|---|
| before | set | 400 |
| 2.5 s after the sheet was rewritten | set: not refreshed | 400: the new rule not applied |
| 2.5 s after `main.qwe` was touched | gone: refreshed | 700 |

The guide says as much (`--watch`: "reload when the program or a `use`d file changes, and
refresh open pages"), so it is not a bug; but it is the loop Tailwind's users live in. Adding
a class to a `.qwe` file refreshes the page at once, while Tailwind writes the sheet a moment
later: the refreshed page can come before the class's rule, and nothing refreshes it again
when the rule comes. The person sees their class do nothing, and refreshes by hand.

No repro in `repros/`: `run.mjs` cannot change a file while a page is open. The probe is the
`drive.mjs` script described above, with a component and the `file()` route.

**Suggestion:** under `--watch`, watch the files the program has served (it knows them: each
went out through `file()`, with its `ETag`), and when a stylesheet changes, tell open pages
on the `--watch` connection to swap that `<link>` for one with a new query (`app.css?v=2`),
without reloading the page or losing its state, as Vite does for CSS; for any other file, the
refresh it does for a `.qwe` file.

### Q-079. No step where CSS is made: Tailwind, PostCSS and the like run beside Quillo, and a component's `style` cannot be processed

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the guide (`quillo run`, `--watch`, `quillo build`, Static files, `style`) | – |

Nothing in Quillo runs a program of another's: `quillo build` builds `.qwer` (Rust), `--watch`
reloads `.qwe`, and a `style` in a component (FEEDBACK8's fix, Q-067) is CSS as it is written.
So, for Tailwind:

| | |
|---|---|
| Starting it | by hand, a second process beside `quillo run`; its log apart from Quillo's; stopping it by hand |
| `@apply`, `theme()`, `@layer` | only in the sheet Tailwind builds, not in a component's `style`, which no tool sees |
| Classes it finds | those written out in a string, as anywhere (`` `bg-${color}-500` `` never, Tailwind's own rule). Quillo's shorthand (`<div.flex.p-4>`, `<p.w-1/2>`) **not checked**: the scanner may take `div.flex.p-4` for one token |
| Production | the built sheet served with `file()`, its name changing with its content for `{ cache: "1y" }`: a step the deploy has to add |

Quillo already knows each class a program writes, in markup and in strings (that is how the
`quillo check` warning of FEEDBACK8's Q-065 fix works), and which components a page shows
(its bundle and its sheet, Q-062 and Q-068).

No repro: what is missing.

**Suggestion,** smallest first:

1. A process run with `--watch` and stopped with it, its lines marked as a service's are:
   `quillo run main.qwe --watch --with "npx tailwindcss -i src/app.css -o public/app.css --watch"`,
   with Q-078's refresh for what it writes.
2. A component's `style` passed through a command the program names
   (`style_with("npx tailwindcss --stdin")`, or a PostCSS config beside the program), so
   `@apply` works where the component's CSS is.
3. A decision for later: utility classes made by Quillo itself, from the classes it already
   knows each page writes, with no Node and no scanner to miss the shorthand.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `npx --yes tailwindcss@3.4.17 -i in.css -o app.css` on a program with shorthand classes, to see which ones it finds | The one way to know what Tailwind's scanner reads | Refused by the session's permission check (downloading and running a package); not worked around. The shorthand question stays open: someone with Tailwind installed can answer it in one command | 1 command |
| The probe's folder, made in the same command | One step | The refused command made nothing, so the next one, in that folder, failed with "no such file or directory" | 1 run |
| "`--watch` reloads when a served file changes" (assumed while answering) | It refreshes for `.qwe` files | It does not for a stylesheet: measured, and Q-078 | 1 probe |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A stylesheet another tool wrote, served with `file()` | `ETag`, `no-cache`: the next page load gets the new sheet |
| `main.qwe` touched under `--watch` | ``quillo: reloaded main.qwe (2 routes)``, the open page refreshed, the new rules applied |
