# Feedback from building a forum in Quillo

Notes from [forum/](../../app/forum), a forum of about 3,200 lines: accounts, live threads, votes,
reactions, accepted answers, polls, @mentions with suggestions, following, bookmarks,
unread tracking, notifications, private messages, image uploads, full-text search, and a
moderators' report queue with suspensions. Everything below was checked on
**quillo 0.5.0 (f1695a3)**, most severe first. Issues from earlier versions of this note
that are now fixed are left out.

## Status on 1cf047c

Rechecked on **quillo 0.5.0 (1cf047c)**, with each issue's repro (`node feedback/run.mjs`)
and the forum's 43 browser checks, run with prefetching on.

| § | Issue | Status |
|---|---|---|
| 1 | Refused prefetch answered 204, so links to routes that write do nothing (Q-002); each refusal logged as an error (Q-003) | fixed: answered 503, and not logged |
| 2 | A second definition silently replaces the first (Q-004); a function named like a builtin replaces it (Q-005) | fixed: an error naming both places; a warning |
| 3 | `use` underdocumented (Q-006) | fixed: "Files of a program" in the guide. The editor's per-file checking was not rechecked |
| 4 | `quillo check` counts only the main file (Q-007) | fixed: `25 routes, 138 functions, 14 components, in 18 files` |
| 5 | The "newer Quillo" note follows every parse error (Q-008) | **open** |
| 6 | A component cannot set an element's field (Q-009) | fixed. The forum now clears its image picker, so the same image can be picked twice |
| 7 | `e.preventDefault()`, and writing in a loader, missing from the guide (Q-010) | fixed in the guide |
| 8 | A removed builtin is just an `unknown name` (Q-037) | **open**, new: see §8 |

The forum needed one change for 1cf047c: `rate_limit` was removed (205d7cd), so its two
limits (images an hour, messages a minute) now count rows in its own `uploads` and
`messages` tables.

What worked well: `server fn` removed the API layer, and `live` with `watch`,
`unwatched` and `publish` gave each thread, inbox and poll its own topic, so a reply
wakes only the pages showing that thread. `quillo check` caught every effect mistake
before the program ran. `re`, `save_upload`, `image.info`/`image.convert` and
`rate_limit` covered the new features with no `.qwer`.

## 1. Bug: in Chrome and Edge, a page whose route writes cannot be opened from a link

Pointing at a link prefetches it. Chrome and Edge do this with speculation rules. A
route that writes (the forum counts a thread's views, and marks its posts read) has its
prefetch refused with **204** ([src/server.rs:297](../../src/server.rs#L297)). The browser
takes any 2xx as a prefetch that worked, so the click is answered with that empty 204,
and a 204 to a navigation leaves the browser where it was. Nothing happens, and the
server never sees the click.

[USERGUIDE.md:1240](../../USERGUIDE.md#L1240) promises the opposite: "The page is then made,
as usual, when the link is followed." Every forum thread link was dead until the forum
was run with `--no-prefetch`.

To reproduce: a route that runs `db.exec`, a page linking to it, Chrome. Point at the
link, then click it.

A server standing in for Quillo refused the prefetch with different statuses; Chrome,
headless, real mouse events:

| Refused prefetch answered with | After the click |
|---|---|
| 204 (Quillo now) | stays on the page; the server never sees the navigation |
| 503 | the prefetch is dropped and the page loads normally |
| 409 | the same as 503 |

**Fix:** answer a refused prefetch with a non-2xx status (503 with `no-store`), and say
so at USERGUIDE.md:1240. The `fetch` prefetch for other browsers is unaffected, since
the answer is already `no-store`.

Also: each refusal is logged as an error (`quillo: error: line 140: not while the page
is prefetched: db.exec`), once or twice for every link pointed at. It is expected, so it
belongs at a debug level, or nowhere.

## 2. A second definition silently replaces the first

No error and no warning from `quillo check` or `quillo run`, whatever the two definitions are:

| Program | Prints |
|---|---|
| `lib/x.qwe` and `lib/y.qwe` both define `fn helper()`, both `use`d | y's (`2`) |
| `main.qwe` also defines `fn helper()` | main's (`3`) |
| One file defines `fn helper()` twice | the second |
| `let helper = 5` beside a `use`d `fn helper()` | `5` |
| `fn len(x) = 99` | `len([1])` is `99`, for the whole program |

In a program of several files this is the most likely way to run the wrong code without
noticing: two files that each want a small `recent()` or `format()` helper, and one
quietly wins. Which one wins depends on the order of `use` lines. (`fn reply` is refused,
with a good message, because `reply` is a keyword.)

**Suggestion:** an error for a name defined twice, naming both places (`helper is
defined in lib/x.qwe:1 and lib/y.qwe:1`), and a warning for one that hides a builtin.

## 3. `use` needs more than one line in the guide

All the guide says is "`use "lib.qwe"` runs another file once, into the global scope."
How it actually behaves was found by trying:

- Every `use`d file, including a file `use`d by a `use`d file, adds to one set of names.
  The order of `use` lines does not matter, and names reach every way: a file can call
  a function from a file only `main.qwe` `use`s.
- So a file is valid only inside a program. `lib/c.qwe` checks fine in a program that
  also `use`s `lib/a.qwe`, and fails with `unknown name` in one that does not.
- A path is resolved from the program's folder, not from the file that writes it:
  `lib/b.qwe` must write `use "lib/c.qwe"`, not `use "c.qwe"`.
- The same file `use`d twice runs once.

These are reasonable rules; they only need saying. Two consequences are worth more than
a sentence:

- **The VS Code language server checks a file on its own**, so every file in `ui/` shows
  red `unknown name` errors for views and functions from its sibling files. Only
  `quillo check main.qwe` is right. Checking each file within the program that uses it
  would fix it.
- **Nothing shows where a name comes from.** `ui/editor.qwe` calls `find_people` (from
  `lib/forum.qwe`) and `<Avatar>` (from `ui/page.qwe`) with nothing in the file saying
  so. Paths relative to the file that writes them, and later named imports
  (`use "lib/forum.qwe" { find_people }`) or `pub` on what a file offers, would let a
  reader, the language server and `quillo check` follow every name to its file, and keep
  helpers private.

## 4. `quillo check` counts only the main file

`quillo check forum/main.qwe` says `ok (25 routes, 2 functions, 0 components)`. The
program has 14 components, 31 views and about 100 functions, all in `use`d files. The counts
would be more useful for the whole program, or labelled as `main.qwe`'s own.

## 5. The "newer Quillo" note follows every parse error

Since f1695a3 a parse error ends with "note: this is quillo 0.5.0 (f1695a3, …); code
written for a newer Quillo fails to parse like this". That helps when the code really is
newer than the binary, which was the problem it fixes. But it also follows errors that
have nothing to do with versions: `fn reply(x)` ("`reply` is a keyword"), and
`e.target.value = ""` ("only variables can be assigned to"). It sends you looking for a
version problem that is not there. Show it only for an unknown keyword or syntax.

## 6. A component cannot set an element's property

`e.target.value = ""` in an event handler, or `el.value = "x"` in `@mount`, is refused:
"only variables can be assigned to". Methods can be called (`el.focus()`), and the guide
says page objects "have their properties and methods as fields", but fields cannot be
set. The forum wanted it to clear its image picker after an upload: without that,
picking the same image again fires no `change` and does nothing, and the only way
around it is a `js(...)` helper. Allow assignment to a page
object's field, with the same checks as method calls, or say in the guide that it is
not possible and what to do instead.

## 7. Small gaps in the guide

- **`e.preventDefault()` works in a handler** (the forum's @mention suggestions use it
  for arrow keys, Enter and Tab, and the editor uses it for pasted and dropped images),
  but the guide never says so. Its list of what a handler's `e` has could add it, with
  the caveat that it must come before anything awaited.
- **Doing something when live data arrives.** A component cannot call a `server fn`
  from a statement that follows state (the error says so clearly). The forum marks
  posts and messages read inside their loaders instead, which works, since a loader
  runs again with the reader's cookies. That pattern is worth a sentence in Live data.
  Today it also runs into item 1, since the loader's write refuses the prefetch.

## 8. A removed builtin is reported as an unknown name

205d7cd removed rate limits, the date module and more, "for programs to do as they need".
The forum, built before that, stopped with no hint of why:

```
forum/lib/uploads.qwe:15: error: unknown name `rate_limit`
```

`date.today()` is worse: `unknown name date (did you mean take?)`. Someone who learned
Quillo from an older guide, or an AI model that saw one, will meet these.

**Suggestion:** keep a short list of removed names, and say so in the error:
"`rate_limit` was removed in 0.5.0: count requests in your own table" or "the date
module was removed in 0.5.0: dates are ISO text, as databases give them, and sort as
strings". Repro:
[feedback/repros/Q-037-removed-builtin-unknown-name.qwe](../repros/Q-037-removed-builtin-unknown-name.qwe).
