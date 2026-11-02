# Feedback from starting a new program: a starter repository, then `quillo new --minimal`

| | |
|---|---|
| App | None. This is a probe of how a new Quillo program gets started, and the design of the option asked for: a `quillo new` that writes only the binary for this machine and the folders, each with a sample `.qwe`; then the same files as a starter repository, `git clone https://github.com/quillolang/starter.git`, which was made, tested and published during this report. The sample (3 files, 37 lines) was written by hand in a scratch folder and run there |
| Quillo | 0.6.0 (378e64b, 2026-11-01), branch `feedback29-fixes`, `./target/release/quillo` |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, `quillo fix`, `quillo run` and curl; headless Chrome (`feedback/tools/chrome.mjs`) clicking the sample's component; a `git clone` of a program with an empty folder; a starter repository made locally and cloned three ways (plain, `--depth 1`, `git archive`); its `get-quillo.sh` against the real v0.6.0 release and a fake one served as `file://`, with a fake `uname`, `sysctl` and `getconf`; `strings` on the Linux build and a listing of the Windows zip; the sample under the older `~/.cargo/bin/quillo`, 0.6.0 (9e8c2fb, 2026-10-31); `gh release view v0.6.0` for the release's file names; `otool -L` and `xattr` on the binary and on a copy of it; reading [src/main.rs](../../src/main.rs), [src/migrate.rs](../../src/migrate.rs), [.github/workflows/release.yml](../../.github/workflows/release.yml), [mobile/ios/build.sh](../../mobile/ios/build.sh), README.md, USERGUIDE.md and the VS Code extension's package.json |

No core change was made, as asked. `quillo new` does not exist yet, and this report is its design, along with the design of a starter repository, which needs no core change at all. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched. Numbers start at Q-422, because FEEDBACK28.md's additions (28a6b3d) took Q-415 to Q-421.

What worked well: `quillo run` and `quillo check` with no argument, in a program's folder, take its `main.qwe`. In an empty folder, `quillo run` says what to do: "no program: name a .qwe file, or add a main.qwe here". The three-file sample below was written from the guide. It passes `quillo check` (`ok (2 routes, 2 functions, 1 components, in 3 files)`), and `quillo fix` has nothing to change. It serves `/` and `/hello/Ana`. In Chrome its component resumed from the server's HTML and counted two clicks, with no errors and no mismatches. The binary needs nothing outside the system: `otool -L` lists only `/usr/lib` and system frameworks. So a copy runs the same as the original: `.quillo/bin/quillo check` printed the same line.

## The answer

There are two ways to get the same files, and the first needs no core change:

1. **Now: a starter repository**, `git clone --depth 1 --branch v0.6.0 https://github.com/quillolang/starter.git shop`. It holds the `--minimal` files below, a `.gitignore`, and a script that downloads the binary for this machine. It works with the 0.6.0 release people can download today, if it is tagged per release and written in 0.6's form (see [A starter repository](#a-starter-repository-git-clone)).
2. **Later: `quillo new --minimal`**, which writes the same files from the binary itself. It needs no network, no version tag and no download script, because the files and the binary always come from the same build.

The option is `quillo new <folder> --minimal`, with `--binary` beside it:

```
quillo new shop                      the to-do app, as app/ is: main.qwe, lib/, ui/, migrations/, public/
quillo new shop --minimal            main.qwe, lib/greet.qwe, ui/page.qwe and a .gitignore; no database, CSS or JavaScript
quillo new shop --minimal --binary   the same, and this quillo copied to .quillo/bin/quillo (quillo.exe on Windows)
```

```
shop/
  .gitignore
  main.qwe            the program: the files it uses and its routes
  lib/greet.qwe       a function, for the server and the browser
  ui/page.qwe         the frame every page uses, and a component
  .quillo/bin/quillo  with --binary only: the quillo that ran `new`, for this OS and CPU
```

`--minimal` has no `migrations/` or `public/`. Neither folder holds a `.qwe`, and git keeps no empty folder (Q-423). A run of it writes no file: there is no `db.open`, so there is no `app.db`.

`--binary` is a separate flag because most people already have `quillo` on their PATH, and the copy is 25.5 MB per program, for one OS and CPU. It goes in `.quillo/bin/` and not beside `main.qwe`. `mobile/ios/build.sh` bundles the program folder whole except `./.quillo/*` and databases (Q-407). Android leaves out dotfolders (Q-408). The `.gitignore` leaves out `.quillo/`, and `quillo build` already keeps `.quillo/native` there.

The name is `--minimal`. `--bare` is git's word for a repository with no working tree. `--empty` would be wrong, since the folders have samples. Cargo's `--bin`/`--lib` do not fit, because every Quillo program is an app, and a library is a file another program `use`s.

## A starter repository: `git clone`

`quillolang/starter` did not exist (`gh repo view`: "Could not resolve to a Repository"). It was made and tested as a local repository, then created on 2026-11-02 at [github.com/quillolang/starter](https://github.com/quillolang/starter), public and marked as a template. `main` and the plain tag `v0.6.0` are both at `7f5ef1f`. The tag first marked `d88480a`, and was moved at the user's word, to `0188b63` once the run scripts were in, to `ef1cae7` with the samples, to `f5f7b1e` with the note kept in the browser, to `d6587cc` with the note and the tasks in SQLite, to `556ab28` with them live, and to `7f5ef1f` with a meta description on every page. The user said to keep moving it to each change that passes CI until the next Quillo release has a tag of its own. The local clone is `~/dev/quillo-starter`.

```
starter/
  README.md                    how to start, with quillo installed or not, and where to go next
  .gitignore                   *.db, *.db-wal, *.db-shm, .env, .quillo/
  main.qwe                     the --minimal files, with `use` lines in 0.6's form
  lib/greet.qwe
  ui/page.qwe
  get-quillo.sh                the release's quillo for this machine, into .quillo/bin/, checked against its SHA-256,
                               and run.sh beside it: ./run.sh --watch runs main.qwe with that quillo
  get-quillo.ps1               the same, on Windows, with run.cmd
  .github/workflows/check.yml  on Linux, macOS and Windows: get-quillo, `quillo check`, `GET /`;
                               in quillolang/starter only, the starter's own page and route as well
```

There are three ways to start from it:

```sh
git clone --depth 1 --branch v0.6.0 https://github.com/quillolang/starter.git shop
cd shop && rm -rf .git && git init          # a history of its own, and no origin
quillo run --watch                          # or: sh get-quillo.sh, then .quillo/bin/quillo run --watch
```

- `gh repo create shop --template quillolang/starter --clone`, or GitHub's "Use this template" button, once the repository is marked as a template: a new repository with the starter's files in a first commit of its own, and nothing pointing back at the starter.
- Without git: `curl -L https://github.com/quillolang/starter/archive/refs/tags/v0.6.0.tar.gz | tar xz`, the files only.

The to-do app is not a second flavour of the starter. The README already lists "a todo app" in [quillolang/examples](https://github.com/quillolang/examples), so the starter is the minimal app and nothing else, and needs no branches.

### What the tests found

`get-quillo.sh` was run against the real v0.6.0 release, and against a fake release served as `file://` for the paths that go wrong. A fake `uname`, `sysctl` and `getconf` stood in for other machines.

| Case | Result | Checked |
|---|---|---|
| The starter under the release people download, v0.6.0 (991e96c), put in place by `get-quillo.sh` | `quillo check` with no argument: `ok (2 routes, 2 functions, 1 components, in 3 files)`. `/` answers 200 with the Counter island, and `/hello/Ana` answers `Hello, Ana!`. In Chrome the button read `Clicked 0 times`, then `Clicked 2 times`, with no problems and no mismatches. Under `--watch`, saving `lib/greet.qwe` turned `/hello/Ana` into `Hi, Ana!` (`reloaded main.qwe (2 routes)`) | run |
| The same files under 9e8c2fb and 378e64b | ok on both; on 378e64b, `quillo fix` reports `nothing to fix` | run |
| The sample in `main`'s form | `import lib/greet.qwe use greeting` fails on 9e8c2fb: `main.qwe:9: error: expected end of statement, found lib`. v0.6.0's parser has no `import` either, since it came in the 60 commits after the tag. So the starter keeps `use lib/greet.qwe` until a release has `import`, and the `v0.6.0` tag keeps it for good | run on 9e8c2fb; read on v0.6.0 |
| `quillo fix` in the workflow | v0.6.0 has no `fix`, which came after the tag: `quillo fix main.qwe` prints the usage and exits 2. So the workflow runs `check` and serves the program. A `fix` step, followed by `git diff --exit-code`, goes in with the first starter tag whose release has `fix` | run on v0.6.0 |
| A new user who follows the README with no quillo on the PATH | `git clone --depth 1 --branch v0.6.0`, then `rm -rf .git && git init` (no remote left, 8 files untracked), then `sh get-quillo.sh` (3.6 s), then `check`, then the workflow's Serve and starter's-page steps, run by bash: all passed. After a run, `git status` shows the 8 files only, with no `.quillo/` and no database | run |
| History and `origin` | a plain clone carries the starter's commits (2 of 2) and `origin` pointing at it; `--depth 1` keeps one commit and the same `origin`. For anyone else, `git push` is refused. For the starter's owners it is accepted: an app started from the starter by quillolang's own account would push into `quillolang/starter`. Hence the README's `rm -rf .git && git init`, or the template button | run on a local starter |
| The tag | an annotated tag cloned with `--depth 1 --branch` printed `warning: refs/tags/v0.6.0 ... is not a commit!` (git 2.54). A plain tag does not, so the tags are plain. Either way git prints its detached-HEAD advice, which the README's `rm -rf .git` makes moot | run |
| `run.sh`, which get-quillo writes (added in `5b23e8b`, at the user's request) | `exec`s `.quillo/bin/quillo run main.qwe "$@"` from its own folder, so the process is quillo itself and a `kill` reaches it. It worked with no options under macOS's `/bin/sh` (bash 3.2, `set -u` with an empty `"$@"`), taking `PORT` from the environment; from another folder, through a path with a space; and with `--watch` (a save reloaded). With `.quillo/` removed it ran `get-quillo.sh` first. A `run.sh` already there, edited, is kept. `run.sh` and `run.cmd` are git-ignored, as the binary is | run |
| `run.cmd`, its Windows counterpart | a `.cmd` rather than a `.ps1`, so cmd and PowerShell run it with no execution policy to get past. It uses a `goto` rather than an `if ( ... )` block, which a folder such as `C:\Program Files (x86)\...` would end early, and is written with CRLF, which cmd needs to find a label. On windows-latest, `get-quillo.ps1` wrote it and `cmd //c run.cmd --port 8080 --log off` served the program: `GET /` answered 200, and the starter's page and `/hello/Ana` were as written (run 41332390951, at `0188b63`) | CI |
| `get-quillo.sh` run twice | the second run prints `quillo 0.6.0 (991e96c, 2026-10-29) is already in .quillo/bin` and downloads nothing | run |
| Another version over one already there | replaced, 0.0.1 by 0.6.0. The new file is moved in whole, so a quillo running from `.quillo/bin/` is not overwritten as it runs | run |
| A release that does not exist (`QUILLO_VERSION=v9.9.9`) | curl's own `404` line, then `cannot download .../quillo-v9.9.9-aarch64-apple-darwin.tar.gz (is v9.9.9 a Quillo release?)`, exit 1 | run |
| A tampered archive | `does not match its SHA-256; nothing was installed`, exit 1, and no `.quillo/` made | run |
| No `sha256sum` on the PATH | `shasum -a 256 -c` is used | run |
| Run from another folder, or from a folder with a space in its name | the binary is installed beside the script | run |
| Which build is chosen | a Rosetta shell on Apple silicon (`uname -m` x86_64, `hw.optional.arm64` 1) gets `aarch64-apple-darwin`; an Intel Mac `x86_64-apple-darwin`; Linux on x86_64 or aarch64 the `-gnu` builds. Linux on armv7l and FreeBSD are told `has no build for ...; build it from source instead`, and Git Bash on Windows is told `run get-quillo.ps1 in PowerShell` | run, with a fake `uname` and `sysctl` |
| Linux's glibc | the x86-64 build needs glibc 2.34 (Q-424). The script reads `getconf GNU_LIBC_VERSION`: 2.28, 2.31 and 2.33 are told they need 2.34 or later; 2.34, 2.35, 2.40 and 3.0 install; a system with no glibc (Alpine) is told so | run, with a fake `getconf` |
| The macOS download | Mach-O arm64, 24.1 MB. Its only attribute is `com.apple.provenance`: curl sets no `com.apple.quarantine` | run |
| The Windows zip | `quillo-v0.6.0-x86_64-pc-windows-msvc/quillo.exe` (26.8 MB), where `get-quillo.ps1` looks for it. Its `.sha256` line has `*` before the file name (sha256sum's binary mode, in Git Bash), where the others have two spaces. `shasum -c` takes both, and the script reads only the first field | downloaded and checked here; `get-quillo.ps1` run in CI under Windows PowerShell: `quillo 0.6.0 (991e96c, 2026-10-29), in .quillo\bin` |
| Windows on ARM | `get-quillo.ps1` takes the x86-64 build, which Windows 11 runs under emulation | not run |
| The workflow | green on ubuntu-latest, macos-latest and windows-latest, for the push of `main` and for the tag (runs 41330477718 and 41330477802), and again with the run scripts (41332390951). The program is served and asked in one step, because on Windows a process a step starts ends with that step. On each system, get-quillo installed 0.6.0, `check` was ok, `GET /` answered 200, and the starter's-page step passed | CI |
| From GitHub | `git clone --depth 1 --branch v0.6.0 https://github.com/quillolang/starter.git` gives no warning, and `get-quillo.sh` and `check` are ok. The tag's tarball (`/archive/refs/tags/v0.6.0.tar.gz`) unpacks to `starter-0.6.0/` with the 8 files, `get-quillo.sh` still executable | run |
| The tag's tarball (`git archive --prefix=starter-0.6.0/ v0.6.0`, the form GitHub serves) | the 8 files, and `get-quillo.sh` still executable | run |
| `.git/` in every app started this way | the iOS build bundles it (Q-407) | seen again |
| The phone builds | README.md, both scripts and `.github/` go into an iOS app too, since build.sh leaves out only databases, `uploads/` and `.quillo/`. That is harmless, but a phone app made from the starter carries them | read |
| Keeping it right | on each Quillo release: the new version in `get-quillo.sh`, `get-quillo.ps1` and the README's clone line; the workflow green on all three systems; then a plain tag with the release's name. Quillo's release workflow can start it | design |
| Offline | needs the network; `quillo new` would not | |

### The samples: `hello`, `spa` and `ssr`, in Quillo UI's theme

Added in `a8dbe09`, at the user's request. `samples/` holds three apps. `get-quillo` asks which one (a numbered menu in a terminal), or takes its name (`sh get-quillo.sh spa`). It copies that sample into the folder and removes `samples/`. Once the folder has a `main.qwe`, it only fetches quillo.

| Sample | What it shows |
|---|---|
| `hello` | a page, a component (a counter) and a function both sides use |
| `spa` | a single-page app, with four tabs as links (`url()`): no reload, Back and Forward, a reload or a shared link renders the tab on the server, an unknown path is a 404. A note kept in the App component while you move between tabs. The note and the tasks are in Quillo's SQLite (`app.db`, a migration), through server fns, so a reload or a restart keeps them (`d6587cc`, at the user's request, after a first version in localStorage, `f5f7b1e`). Both are live (`556ab28`, at the user's request): a change on one page shows on every other open page |
| `ssr` | notes in SQLite (a migration), pages made on the server, forms that redirect (303) after posting and answer 422 with what was typed. No component, so it works with JavaScript off |

Each vendors Quillo UI's `ui/theme.qwe` as it is (kit `6e34311`), the theme the kit's README calls the one every page needs. The kit's components, taken file by file, cannot be used: each starts with `import ui/theme.qwe use ...`, which v0.6.0 does not have (the kit's README says they need "the release after 0.6"). Each sample has a theme switch, System, Light or Dark. It is kept in a cookie, so the server puts `ui-light` or `ui-dark` on `<html>` as it renders, with no flash of the other theme. In `hello` and `spa` the switch is a component that changes the class at once and keeps the choice through a `server fn`. In `ssr` it is a form that posts to `/theme`, and its way back is checked to be a path on the site.

| Case | Result | Checked |
|---|---|---|
| `quillo check` on each sample | ok on v0.6.0 (991e96c) and 378e64b, no warning; `quillo fix` on a copy: nothing to fix | run |
| `get-quillo.sh` choosing | no name and no terminal: hello, said so. `spa`: copied, `samples/` removed. A name that is no sample, `--help` or two names: exit 2. A name where `main.qwe` is: exit 1. No `main.qwe` and no `samples/`: exit 1. A `lib/` already there: exit 1, nothing copied, the file kept | run |
| The menu, through a pseudo-terminal (Python's `pty`) | `2` and `spa` give spa, `3` and `ssr` give ssr, Enter gives hello, `9` gives `no sample "9"` and exit 2 | run |
| Each sample made by `get-quillo` with the real v0.6.0, served by `run.sh` | `.github/starter-test.sh`: hello 17 checks, spa 30, ssr 34, all passed | run |
| hello and spa in Chrome (v0.6.0) | Dark sets `ui-dark` at once with no reload, and the page is dark; a reload renders it from the server; System with the system dark is dark, Light then is light; the counter counts. spa: the tabs change with no reload; a task added, an empty one gets the server's reason; the note is kept across tabs (17 characters); Back; dark from tab to tab; a reload on `/tasks` renders that tab, dark; an unknown path shows Not found. Nothing made again, no errors, no mismatches. 15 and 28 checks | run |
| ssr in Chrome with JavaScript off (`Emulation.setScriptExecutionDisabled`) | the theme form posts and comes back dark; a note saved lands on its page, line breaks kept; Light from the note's page returns to it; Delete returns to the list. 10 checks | run |
| The theme cookie | not `Secure`, so it holds over plain HTTP. A value that is no theme reads as System. A call to `save_theme` from another site gets 403 | run |
| The ssr form's way back | `//evil.example`, `https://evil.example`, `/\evil.example`, `javascript:alert(1)` and an empty one all go to `/`; a post from another site gets 403 | run |
| Markup in a task or a note's title | shown as text, in the page and in the island's state | run |
| The README's way with quillo installed (`cp -R samples/spa/. . && rm -rf samples`) | `quillo check`: ok | run |
| The spa note in localStorage (`f5f7b1e`, replaced by SQLite in `d6587cc`), in Chrome on v0.6.0 and 378e64b | a fresh browser starts empty; kept as typed; after a reload on `/notes`, after a reload on Home and the tab, after another tab, and after the server restarted; markup comes back as text; nothing made again, no mismatches. With localStorage throwing (storage off), the note still works for the page, with no error. 12 checks on each build. Loaded in App's `@mount` and saved by one `@input` beside `value={note}`, not `bind:value` with an `@input` too (Q-094 was fixed after v0.6.0). The key is the starter's own, `starter.spa.note`: every program served at one address shares localStorage | run |
| The spa note in SQLite, in Chrome on v0.6.0 | 15 keys typed 40 ms apart make one save, half a second after the last (`every(500, ...)` under `note != saved`, which each key starts again). The page says Saving…, then Saved. The server puts the note in the page it makes, so a reload has it at once. Typing and moving to another tab at once still saves (the `every` is App's). A server restart keeps the note and a ticked task. Two bursts make two saves, the last text kept. 15 checks | run |
| The note and the tasks live, across two people (two browser contexts) in Chrome, on v0.6.0 and 378e64b | a task added on A shows on A and on B with no reload; ticked on B, it shows on A. A's note, saved, shows on B, which says Saved. While B types, A saves: B's text stays as typed (a page takes a note from elsewhere only when it has no typing of its own not yet saved), then B's save, the last, is the note on A and in the database: the whole note's last save wins, the two are not merged. No errors, no mismatches on either. 13 checks on each build | run |
| The earlier suites, with the note and tasks live | the SQLite note's 15 checks, the slow server's 6, the refused save's 7, the spa's 28 in Chrome, `starter-test.sh` spa's 36: all passed. During the plain run's deliberate restart, Chrome logs the live connection refused until the server is back, as expected | run |
| A slow server (a test copy whose save sleeps 1.2 s for a text of even length) | one save at a time (`saving`), each with the newest text: the database ends with the last text, in 3 calls. A tick of `every` waits for a server fn's answer on v0.6.0, which this needs (probed: a 300 ms server fn gave its value, not a Promise) | run |
| A save refused (a test copy refusing a text that ends with `!`) | `Not saved: <the reason>`; not tried again in 2 s while the note stays the same; one key more and it is saved | run |
| Lighthouse 13.6.0's SEO (the user saw 90) | 90 on every page of spa and hello: the one audit failing was `meta-description`, "Document does not have a meta description". With one on every page (`7f5ef1f`): 100 on spa's four tabs, hello, ssr's list and three of its notes. spa's comes from the path, on the page the server makes, as a search engine reads it, and its 404 has one too. An ssr note's is its text on one line, cut near 155 characters (`split`, `trim`, `join`, `slice`), its title when it has none; quotes and markup in it are escaped in the attribute. `starter-test.sh` checks them | Lighthouse |
| CI: each sample made by get-quillo on ubuntu-latest, macos-latest and windows-latest (PowerShell's menu code, run.cmd), served, and checked by `starter-test.sh` | all 9 jobs passed (run 41334889237, at `ef1cae7`; again at `d6587cc`, run 41336279542, with the spa in SQLite) | CI |


## Seen again

| ID | What it was | Here |
|---|---|---|
| Q-407 | the iOS app carries the program folder whole, `.env` and `.git/` included | a binary placed next to `main.qwe` would ship in every iOS build, 25.5 MB of macOS code inside a phone app. [build.sh:58](../../mobile/ios/build.sh#L58) leaves out `./.quillo/*`, so `--binary`'s copy goes there. And every app started from a clone of a starter has a `.git/`, which ships too |
| Q-408 | Android leaves out every dotfile and `_` folder | `.quillo/bin/` is left out on Android too, so both phone builds skip the copy |

## New issues

### Q-422. There is no way to start a program: no `quillo new` or `quillo init`, and the release has no sample app

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, docs | trying it; reading release.yml and README.md | 1 |

`quillo new myapp` and `quillo init` print the usage and exit 2, as any unknown word does ([src/main.rs:339](../../src/main.rs#L339)):

```sh
$ quillo new myapp; echo $?
Quillo — a small web language

usage:
  quillo run <file.qwe> [options]   run a program; serve it if it has routes
  ...
2
```

So the only ways to start are writing `main.qwe` from the guide or copying `app/`. And `app/` exists only in the source. A release archive holds `quillo`, README.md and LICENSE ([release.yml:57-65](../../.github/workflows/release.yml#L57-L65)). README's Getting started tells someone with the release to "Run the sample app in [`app/`](app)" ([README.md:230](../../README.md#L230)), which they do not have. In a clone, `app/` also holds the ten apps of this feedback program (arena, bank, clinic, cms, coop, forum, pm, shifts, snake, stays), so copying the folder copies them too. Only eight of its tracked files are the to-do app.

**Suggestion:** first the starter repository ([A starter repository](#a-starter-repository-git-clone)), which needs no core change. It now exists; what is left is README's Getting started pointing to it. Then `quillo new <folder>` with `--minimal` and `--binary`, as above:

- A `Some("new")` arm in `real_main`, with its own line in `USAGE`. Its options are checked as `with_one_program` checks them: an unknown one is told the nearest, a missing folder gets the usage and exit 2, and `-h` prints the usage.
- The to-do template is `app/`'s eight tracked files, embedded with `include_str!("../app/main.qwe")` and the rest, so the sample and what `new` writes cannot differ. The minimal template is the three files below, in `src/new/minimal/`.
- `new` writes into a temporary folder beside the target and renames it at the end, so a failed `new` leaves nothing behind (see the table below).
- `tests/new.rs`: for each template and for `--binary`, `quillo new` into a temporary folder. Then `quillo check` is ok with no warning, `quillo fix` has nothing to fix, `GET /` answers 200, and in `tests/browser/`, the component clicks.
- The docs: `USAGE`; USERGUIDE.md's paragraph on the other commands; README's Getting started (start with `quillo new`, and keep `app/` for a clone); book/, through `assemble.py`; CHANGELOG.md.
- In ~/dev/quillo-editors: the VS Code extension looks for `quillo` on the PATH, then `target/release/quillo` and `target/debug/quillo` in the workspace (the description of `quillo.path`). It should look in `.quillo/bin/quillo` first. The JetBrains plugin's lookup was not read.

### Q-423. An empty `migrations/` works where it was made, and stops the program in a clone

| Kind | Detected by | Attempts |
|---|---|---|
| loud, only after a clone | `git clone` of a probe | 1 |

`db.migrate("migrations")` reads an empty folder as no migrations (`[]`) and a missing folder as an error. Git keeps no empty folder. So a program whose `migrations/` is still empty starts on the machine that made it, and stops in a fresh clone or in CI:

```sh
# main.qwe:  db.open("app.db")  db.migrate("migrations")  print("started"); migrations/ is empty
quillo run main.qwe          # started
git clone probe clone        # git ls-files: main.qwe only
quillo run clone/main.qwe    # quillo: clone/main.qwe:2: db.migrate: cannot read clone/migrations: No such file or directory (os error 2)
```

The error is right to stop on a misspelt folder. But it does not say why a folder that exists on one machine is missing on another. A `quillo new` that wrote an empty `migrations/` would hand this to every new program.

**Suggestion:** keep the error, and when the folder is missing, add the likely reason ([src/migrate.rs:17](../../src/migrate.rs#L17)): "cannot read clone/migrations: there is no such folder (git keeps no empty folder: put the first migration in it, or a `.gitkeep`)". A `.gitkeep` is safe, because `files()` takes only `.sql` files: `migrations/.gitkeep` alone gives `[]`. `quillo new` never writes an empty folder.

### Q-424. The Linux release needs glibc 2.34 or later, and nothing says so

| Kind | Detected by | Attempts |
|---|---|---|
| loud on an older system, docs | `strings` on the release's binary | 1 |

The Linux builds are made on `ubuntu-22.04` ([release.yml:32-33](../../.github/workflows/release.yml#L32-L33)) and linked against its glibc. v0.6.0's x86-64 build has symbols up to `GLIBC_2.34`. So on Ubuntu 20.04 and Debian 11 (glibc 2.31), RHEL 8 (2.28) and Amazon Linux 2 (2.26), all still common on servers, the downloaded quillo will not start. Instead the loader says `GLIBC_2.34' not found`. README's Getting started says "Linux (x86-64 and ARM)" and names no minimum.

```sh
tar xzf quillo-v0.6.0-x86_64-unknown-linux-gnu.tar.gz
strings quillo-v0.6.0-x86_64-unknown-linux-gnu/quillo | grep -oE '^GLIBC_[0-9.]+$' | sort -t. -k2,2n -u | tail -1
# GLIBC_2.34
```

It was not run on such a system: this comes from the symbols. The aarch64 build was not downloaded.

**Suggestion:** say it in README's Getting started: glibc 2.34 or later, so Ubuntu 22.04, Debian 12, RHEL 9 or newer. For servers with less, either add static musl builds (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`), which run on any Linux, Alpine included, or build the `-gnu` ones against an older glibc (cargo-zigbuild's `x86_64-unknown-linux-gnu.2.17`). The starter's `get-quillo.sh` checks for 2.34 today, and would follow whichever is done.

### Q-425. `delete_cookie` is always `Secure`, so a cookie cannot be removed over plain HTTP, even one set with `secure: false`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl on the starter's theme switch, then Chrome at this Mac's network address | 1 |

`set_cookie` takes `secure: false`, and a preference such as a theme uses it, so that it holds when the site is reached over plain HTTP, as a phone on the same network reaches `http://192.168.1.126:8080`. `delete_cookie(name, { path, domain })` has no such option, and always sends `Secure`. Chrome refuses a `Secure` cookie from a plain-HTTP origin other than localhost. So the deletion never arrives, and the old value stays, with no error on either side:

```quillo
route GET /set = { set_cookie("theme", "dark", { secure: false }); "set" }
route GET /del = { delete_cookie("theme"); "deleted" }
route GET /show = cookie("theme") ?? "none"
```

```
quillo run --host 0.0.0.0 main.qwe, then in Chrome /set, /show, /del, /show:
http://192.168.1.126:18810   set, dark, deleted, dark      the cookie is still there
http://127.0.0.1:18810       set, dark, deleted, none
/del answers: set-cookie: theme=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax
```

The server's warning at the time names the wrong thing: "a Secure cookie was sent to http://192.168.1.126:18810 over plain HTTP, which browsers drop: sessions will not stick there", with no session involved. Run on v0.6.0 (991e96c).

The starter's samples keep "system" as a value (`set_cookie("theme", "system", { secure: false })`) rather than deleting the cookie.

**Suggestion:** give `delete_cookie` the options that decide whether a browser takes it, `secure` and `same_site`, as `set_cookie` has. Or have it default to what `set_cookie` would send on this request, `Secure` only where `set_cookie` would set it. Say in the guide that a cookie set with `secure: false` must be deleted the same way. And let the plain-HTTP warning name the cookie it is about.

### Q-426. A program cannot learn the address it listens on, so one that reads its own pages must be told it again

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | building the site search (the `docsearch` plugin), and reading the live unit | 1 |

quillolang.org's search reads the site's own sitemap and pages when it starts. The plugin, `plugins/docsearch.qwer` in the website, does it from a background thread. To do that, the program has to know where it listens. Quillo does not say:

- `--port 4300`, as the live unit gives it (`ExecStart=... quillo run main.qwe --host 127.0.0.1 --port 4300`), is not in the environment. `env("PORT")` is null, and only a `$PORT` given by hand would be there.
- `request` has `path`, `query`, `headers`, `ip` and the rest, but no host or port of its own. Behind a proxy, `Host` is the public name (`quillolang.org`).
- `args()` holds only what follows `--`.

So the site takes a `SEARCH_FROM` setting (`http://127.0.0.1:4300` in its `site.env`), which repeats the unit's `--port` and has to be changed with it. A program that warms its own caches, checks its own pages or sends itself a webhook meets the same thing. Run on 378e64b.

**Suggestion:** `server()` (or fields on `platform()`): `{ host, port, url, base }` as the server bound them, null in a script. `--port` and `--host` could also set `PORT` and `HOST` for the program, as `$PORT` sets the port now.

## The files `--minimal` writes

This is what it writes for `quillo new hello --minimal`, run as written on 378e64b. The folder's name appears once, in `title="hello"`.

```quillo
// main.qwe
// A Quillo program. This file is the program: the files it uses and its routes.
//
//   quillo run --watch      then open http://127.0.0.1:8080
//   quillo check            checks every file it uses, without running
//
//   lib/greet.qwe     functions, for the server and the browser
//   ui/page.qwe       the frame every page uses, and a component

import lib/greet.qwe use greeting
import ui/page.qwe use Page, Counter

route GET / =
    <Page title="hello">
        <h1>{greeting("world")}</h1>
        <Counter start={0}/>
    </Page>

route GET /hello/:name = greeting(name)
```

```quillo
// lib/greet.qwe
// Plain functions: the server and the browser both use them.

fn greeting(name: str) -> str = `Hello, ${name}!`
```

```quillo
// ui/page.qwe
// The frame every page uses, and a component that runs in the browser.

view Page(title: str, children) =
    <html>
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1"/>
            <title>{title}</title>
        </head>
        <body>{children}</body>
    </html>

component Counter(start: int) {
    state count = start
    <button @click={() => { count += 1 }}>Clicked {count} times</button>
}
```

```
# .gitignore
*.db
*.db-wal
*.db-shm
.env
.quillo/
```

It uses `import ... use` rather than a bare `use`, so a new program starts in the form strict modules will require, not with names that only work with a warning.

## What `quillo new` has to get right

| Case | What to do | Checked |
|---|---|---|
| The folder exists and is not empty | refuse, naming what is there. An empty folder, or one holding only `.git`, is fine. `quillo new .` writes into the current folder under the same rule | design |
| The folder is inside another program (a parent folder has a `main.qwe`) | warn. The iOS build leaves a subfolder with its own `main.qwe` out of the parent's app ([build.sh:56](../../mobile/ios/build.sh#L56)), and says so only in one line of its output | read |
| The folder's name in the code | only in `title="..."`, with `\` and `"` escaped. `{` there is literal: `title="Tom {app}"` gives `<title>Tom {app}</title>`, and `\"` gives `&quot;` | run |
| The folder's name in a comment | not written. A newline in a folder name (macOS and Linux allow one) ends the comment, and the rest is code: `// app: two` then `lines` gives `d.qwe:2: error: unknown name lines` | run |
| Names Windows refuses (`con`, `aux`, `nul`, `com1`, a trailing dot or space, `<>:"\|?*`) | refused on every OS, so a program made on a Mac can be cloned on Windows | not run |
| Empty folders | never written (Q-423). `--minimal` has no `migrations/` or `public/`, and the to-do app has a file in each | run |
| `.gitignore` | `*.db`, `*.db-wal`, `*.db-shm` (one run of the to-do app leaves all three), `.env` (the guide: "Keep `.env` out of version control"), `.quillo/` (`quillo build`'s output and `--binary`'s copy). The repository's own .gitignore has all of these but `.env` | run |
| A failure halfway (a full disk, a folder it cannot write) | write into a temporary folder beside the target and rename it at the end, so nothing half-made is left | design |
| `--binary` from a debug build | say so, with the size: `target/debug/quillo` is 91.7 MB, the release 25.5 MB | measured |
| What `--binary` copies | `std::env::current_exe()`, the file itself and not a link to it (a Homebrew `quillo` is a link). The copy keeps the file's attributes: `com.apple.provenance` is on the build and on its copy, so a downloaded binary's `com.apple.quarantine` would come along too | `xattr`; quarantine not run |
| `--binary` and a team on several OSes | the copy is this machine's OS and CPU, and `.quillo/` is not committed, so a teammate on another OS installs quillo. `.quillo/bin/quillo version` says which one the copy is | design |
| After writing | print the next step: `cd hello && quillo run --watch`, or with `--binary`, `.quillo/bin/quillo run --watch` | design |
| The to-do template's comments | `app/main.qwe`'s comments say `quillo run app/main.qwe`. Written into a new folder they would be wrong, so they say `quillo run --watch` (run inside the folder) | read |
| The templates stay right | `tests/new.rs` (Q-422). This report's own first sample failed `quillo check` (below), and a template is what people and models copy | run |

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `let n = start` and `@click={n += 1}` in the sample component | Svelte's habit: a plain `let` is state and an assignment is a handler | `./ui/page.qwe:15: error: expected }, found +=`. Quillo has `state count = start` and `@click={() => { count += 1 }}`, as the guide's Components section shows | 1 check |
| Read exit code 0 from `quillo new myapp 2>&1 \| head` | the pipe's status is `head`'s | run again without the pipe: 2 | 1 run |
| The sample's `import lib/greet.qwe use greeting` | this checkout's guide is `main`'s, and the form is new since v0.6.0 | `main.qwe:9: error: expected end of statement, found lib` on the 0.6.0 build installed here. `use lib/greet.qwe` works on both | 1 check |
| `QUILLO_VERSION=v0.0.1 sh starter-repo/get-quillo.sh`, a test meant for a copy | the copies and the starter had the same script | it replaced the starter's real v0.6.0 with the fake 0.0.1, which became a test of replacing one version with another | 1 download |
| The workflow serving the program in one step and asking it in the next | on Linux and macOS a background process lives on into the next step | it does on those two. On Windows the server was gone (`GET / 000`, curl exit 7), so serving and asking are now one step. In the same step, `curl \| grep -q` became a variable, since GitHub's bash runs with `pipefail` | 1 CI run |
| `quillo fix main.qwe` on the spa sample, as a check | it had said `nothing to fix` until then, so it read like a report | it wrote `import ui/theme_switch.qwe use theme_now`, which v0.6.0 cannot read; replaced by `use ui/theme_switch.qwe`, and `fix` run on copies from then on | 1 check |
| The theme's System as `delete_cookie("theme")` | the natural way to forget a choice | removed nothing over plain HTTP (Q-425); now a value | 1 probe |
| macOS `script` to answer get-quillo's menu | the usual way to give a script a terminal | it sent end-of-input first, so every answer read as Enter; Python's `pty` instead | 1 run |
| The ssr checks' `curl -d back=/notes/2` and `back=//evil.example` in Git Bash on Windows | the same script passed on Linux and macOS | Git Bash rewrote both before curl sent them, into a Windows path and `/evil.example`, and the server rightly sent the first home and took the second as its own path. `MSYS_NO_PATHCONV=1` in the script | 1 CI run |
| `rm -rf $F` in the test loop, with `sh -c` | a fresh folder per run | Claude Code's safety check refused it: a removal of a path made from variables. Each run takes a new folder of its own instead, with nothing removed | 1 command |
| `for p in $paths` to run Lighthouse on each page | bash's habit | this shell is zsh, which does not split an unquoted variable: Lighthouse was given every path as one address and could not load it (`ERRORED_DOCUMENT_REQUEST`); the paths written out instead | 1 run |
| `git tag -a v0.6.0` | an annotated tag, as releases often have | a shallow clone of it printed `refs/tags/v0.6.0 ... is not a commit!`; tagged again as a plain tag | 1 clone |
| The folder's name in the sample's first comment, `// hello: a Quillo program` | the to-do app's comments name its own paths | a name with a newline would turn into code (see the table above), so the template puts the name only in `title` | 1 edit |

## Limitations

- Nothing was built: no core change, as asked. The three files and the `.gitignore` above were written by hand and run on 378e64b. `quillo new` itself, its refusals and its temporary-folder write are design.
- Windows (`quillo.exe`, the names it refuses) is from reading only.
- The quarantine attribute was not tested, because this build was made here and not downloaded.
- GitHub's template button (`gh repo create --template quillolang/starter`) was not tried, because it would make a repository in someone's account.
- `get-quillo.sh` ran for real on macOS (arm64), here and on GitHub's macos-latest, and on GitHub's ubuntu-latest (x86-64). `get-quillo.ps1` ran only on windows-latest. Linux on ARM, Intel Macs, an old glibc and Windows on ARM ran only with fakes, or not at all.
- The `use` form was run on 9e8c2fb, a build two days after v0.6.0, not on v0.6.0 itself. That v0.6.0 takes it comes from the CHANGELOG.md at the tag, whose 0.6.0 section has "Paths without quotes. `use lib/auth.qwe`". Its parser was searched only for `import`, which it does not have.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `quillo run` and `quillo check` with no argument, in the program's folder | take its `main.qwe` |
| `quillo run` in an empty folder | "no program: name a .qwe file, or add a main.qwe here", exit 2 |
| The minimal program | `quillo check`: `ok (2 routes, 2 functions, 1 components, in 3 files)`; `quillo fix`: `nothing to fix` |
| Its routes | `/` answers 200 with the island, its `data-p` holding `{"start":0}`, and `/hello/Ana` answers `Hello, Ana!` |
| Its component in Chrome | `Clicked 0 times`, then `Clicked 2 times` after two clicks; no problems, no mismatches |
| A run of the minimal program | writes no file in its folder |
| `db.migrate` on an empty folder, or one holding only `.gitkeep` | `[]` |
| An older quillo given code in a newer form | the error ends with "note: this is quillo 0.6.0 (9e8c2fb, 2026-10-31), built 2026-10-31 at /Users/restiguay/.cargo/bin/quillo; if the code was written for a newer Quillo, that would explain it" |
| A copy of the binary at `.quillo/bin/quillo` | `version` and `check` give what the original gives |
| The binary's libraries | `/usr/lib/libSystem`, `libobjc`, `libiconv`, and the Foundation, CoreFoundation, Security and SystemConfiguration frameworks only |
