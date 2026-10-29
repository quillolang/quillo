# Finding Quillo's edge cases by building apps

Many apps get built in Quillo, each by an AI model with the same instruction: build
it, bring out the edge cases, and document them. What comes back measures two things:
where Quillo is wrong or missing something, and how a model meets a language it has
never seen.

| | |
|---|---|
| [INDEX.md](INDEX.md) | every known issue, once, with its kind, where it was seen, and its status |
| [repros/](repros) | a runnable repro for each issue that can have one |
| [run.mjs](run.mjs) | runs the repros against the current build: open, fixed or changed |
| [TEMPLATE.md](TEMPLATE.md) | the shape of a report (`reports/FEEDBACKn.md`) |
| [tools/](tools) | headless Chrome, for driving pages and catching what fails silently |

Reports so far: [FEEDBACK.md](reports/FEEDBACK.md) (a forum), [FEEDBACK2.md](reports/FEEDBACK2.md)
(a project manager), [FEEDBACK3.md](reports/FEEDBACK3.md) (a newsroom CMS with a flow and
object designer), [FEEDBACK4.md](reports/FEEDBACK4.md) (a bank made of services, with a
clerk–client desk view), [FEEDBACK5.md](reports/FEEDBACK5.md) (a booking platform on outside
maps, weather and places, built with the Quillo UI kit, and the components it lacked),
[FEEDBACK6.md](reports/FEEDBACK6.md) (no app: whether Quillo can make a video call, the
command line with no program, and a chain of services with a slow link),
[FEEDBACK7.md](reports/FEEDBACK7.md) (no app: how Quillo ships its JavaScript, and how
its bundles are keyed and kept), [FEEDBACK8.md](reports/FEEDBACK8.md) (no app: unused
classes in the five apps, the classes Quillo sets itself, and component CSS that is not
scoped), [FEEDBACK9.md](reports/FEEDBACK9.md) (no app: whether Quillo keeps its keywords,
builtins and modules apart from a program's names, on the server and in the browser; how `run` takes its options; `.env` and a service's settings), [FEEDBACK10.md](reports/FEEDBACK10.md) (no app: one service
scaled to several copies without changing its code, a copy joining while the app runs, and
retries behind a balancer, health and API description through a gateway), [FEEDBACK11.md](reports/FEEDBACK11.md) (no app:
Tailwind and CSS tools beside Quillo, and `--watch` with a generated stylesheet), [FEEDBACK12.md](reports/FEEDBACK12.md) (a real-time
multiplayer game: a server loop, channels, a game as a service, and a simulation with browsers and bots), [FEEDBACK13.md](reports/FEEDBACK13.md) (two-shift scheduling for a
factory of 24 lines: rotating crews, leave and its approval, and live manpower for a week or a month), [FEEDBACK14.md](reports/FEEDBACK14.md) (a credit
cooperative: members, savings, fixed deposits, a loan designer, loans approved by the board, salary deductions from employers by file or API, stores, raffles and events), and
[BUGREPORT1.md](reports/BUGREPORT1.md).

## Each run

1. **Start from the newest Quillo.** `git pull`, `cargo build --release`, and use
   `./target/release/quillo`. An older `quillo` on the `PATH` (and the editor's
   language server) may still be running; `quillo --version` names the commit.
2. **Learn it from the guide.** Read [USERGUIDE.md](../USERGUIDE.md), and an app
   already built here.
3. **Read [INDEX.md](INDEX.md).** Known issues are not news. Note the ones the app
   runs into anyway: how often models trip on the same thing is the signal.
4. **Probe before building on something risky.** Write a few lines and run them
   first, in the server and in the browser.
5. **Build the app** in a folder of its own in `app/` (`app/<name>/`), beside the sample to-do app.
6. **Test it the way a person uses it**, in headless Chrome with real mouse and key
   events (`tools/`), not only with `quillo check`. Look hardest for what fails
   *without a word*:
   - `node feedback/tools/mismatches.mjs <url> <paths…>` on every page: components
     the browser made again because the server's HTML did not match (`remade`), and
     errors Quillo caught.
   - Empty data, one item, many items (thousands of rows), two tabs at once, a
     viewer who may not change things, someone outside, hostile calls to `server fn`s
     (`POST /_quillo/rpc/<name>`), odd input (dates, long text, markup, formulas).
7. **Write the report** from [TEMPLATE.md](TEMPLATE.md) as the next `reports/FEEDBACKn.md`.
   Every finding comes with a repro checked on the version named at the top.
8. **Update the index:**
   - add the app to **Seen** for each known issue it hit;
   - give each new issue the next ID and a row;
   - save its repro in `repros/` (below);
   - run `node feedback/run.mjs --update`.

## Repros

`repros/Q-NNN-short-name.qwe`, or a folder of that name with a `main.qwe` when the
issue needs several files. The first comment lines tell [run.mjs](run.mjs) how to run
it and how to read what comes out:

```quillo
// Q-012 `let` and `for` silently do not bind names like int, str, num, list
// mode: run
// open-when: <builtin
// fixed-when: [5, "a", 6]
// From FEEDBACK2.md §2 (found on 896e758).

let int = 5
...
```

| Line | |
|---|---|
| `mode:` | `check` (what `quillo check` says), `run` (what `quillo run` prints), `cli` (what quillo prints for the `args:` line, in the repro's folder), `serve` (responses to `get:` requests, then the server's log), `browser` (the page in headless Chrome: `probe:` result, `remade`, mismatches, console), `script` (what a shell script beside it prints, for steps a request cannot take: a signal, a file edited while it runs) |
| `args: run --watch` | `cli`: quillo's own arguments, for issues of the command line |
| `get: /path header=value` | `serve`: a request to make; several allowed |
| `page: /path` | `browser`: the page to open |
| `probe: return …` | `browser`: JavaScript run in the page once its components are up; returns JSON |
| `script: steps.sh` | `script`: the script to run in the repro's folder; `$QUILLO` is the build under test |
| `flags: …` | more options for `quillo run` |
| `open-when:` | the bug is there if the output contains this (any of them) |
| `fixed-when:` | fixed if the output contains this (all of them); with no `open-when`, open until then |

A marker between slashes is a regular expression (`/error: line \d+:/`). A repro that
shows neither marker is **changed**: Quillo now does something else, and someone
should look. `--verbose` prints every output.

```sh
node feedback/run.mjs                 # all
node feedback/run.mjs Q-011 Q-013     # some
node feedback/run.mjs --update        # and write Status into INDEX.md
QUILLO=/path/to/quillo node feedback/run.mjs   # another build
```

## Tools

Node 22 or newer, and Chrome (or `QUILLO_CHROME`); nothing to install.

- `tools/chrome.mjs`: the library. `launch({ mismatches })` gives a browser; `page()`
  gives a tab with `go`, `ev` (run JavaScript, get its result), `booted`, `quillo()`
  (resumed, remade, errors), `click`, `drag`, `key`, `type`, `dragDrop` (HTML5 drag and
  drop), `submitForm`, `shot` (a screenshot), and `problems` and `mismatches`, filled
  as the page runs. With `mismatches: true` it stops at each `Mismatch` Quillo's
  runtime throws and records the text or element it wanted and what the server sent.
- `tools/mismatches.mjs <url> <paths…> [--login "<path> <form> name=value …"]`: visits
  each page and reports anything made again, caught, thrown or logged. Exits 1 if any.
- `tools/unused-css.mjs <program folder> [--all]`: the classes a program styles (its
  `public/` sheets and `` css`...` ``) that nothing in it can write, by file and line. A
  class a template may make (`` `ui-tabs-${v}` ``) is "maybe"; Quillo's own (`q-*`, `hl-*`)
  are read from its runtime. A library's classes (`.leaflet-*`) show as unused.
- `tools/service-scale/run.sh`: an app and a computing service, run in one process, as one
  copy, three listed, and behind limer (round-robin, hashed on `x-quillo-call`, a copy joining
  mid-run): calls a second, which copy answered, and calls that ran twice. `LIMER=` for
  limer's binary; the limer steps are skipped without it.
- `tools/arena-sim.mjs <url> [players] [bots,...] [seconds]`: loads Quillo Arena (`app/arena`) with
  real players in headless Chrome and bots, and reports its loop's ticks a second, step time, gaps
  between ticks, snapshot size and the share sent on a stream, and each player's updates a second.
- `tools/drive.mjs test.mjs [--mismatches]`: runs a script that default-exports
  `async ({ browser, page, sleep, ALT, CTRL, META, SHIFT }) => result`.
