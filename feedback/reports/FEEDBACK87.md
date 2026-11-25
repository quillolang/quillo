# Feedback from making "One Binary, One Command", a video on what `quillo run` does

| | |
|---|---|
| App | `~/dev/quillo-videos/videos/212-arch-one-binary/app/main.qwe`: a 29-line page with a `Like` component, two server fns and a count in SQLite (one migration). Made for video 212, the first of a "Quillo system architecture" series (QUEUE 200), by quillo-videos-40: the program was broken once per stage of `quillo run` (lexer, parser, checker, interpreter) to show each stage's own error |
| Quillo | `quillo 0.8.10 (61997ab, 2026-11-22)`, from the release (get-quillo.sh); the repros were also run on the repo's `target/release/quillo`, `0.8.10 (1a4bb9b, 2026-11-24)` |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`, `quillo run`, `quillo docs`, curl, Playwright (Chromium) clicking the page, `node feedback/run.mjs Q-1370 Q-1371` |

What worked well: each stage's error was clear and placed. A curly quote is named as one with what to write (`unexpected character `“`, a curly quote: write `"``); a misspelled closing tag names both tags and the line; a wrong argument count names the function; a misspelled migrations folder passes the check and stops the start with a message that even guesses why (git keeps no empty folder). `quillo run` checks first and refuses a program with errors. Measured on an Apple M5 (busy, median of 11): `quillo check` 8.9 ms, start to the first page 22.4 ms, a save to the new page under `--watch` 40.9 ms, 21.2 MB resident.

Numbers Q-1370 and Q-1371, from the block Q-1370 to Q-1379.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-008 | The "if the code was written for a newer Quillo" note still follows a typo's parse error: a handler's `}` left out before `>♥` (Q-1371 below) | 1 |

## New issues

### Q-1370. A field typo on a value made by a record type, or returned as one, passes `quillo check`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check`, then the page (making a video) | 1 |

The guide says `quillo check` finds misspelled fields, and it does on a plain map (`let lamp = { name: "Desk", watts: 40 }` then `lamp.wats`: "this map has no field `wats` (did you mean `watts`?)") and on a parameter typed with the record (`fn label(lamp: Lamp) = lamp.wats`: the same error). But a value made by the record's constructor, at the top level or inside a fn, or one returned by a fn declared `-> Lamp`, is not followed: check says ok and the page shows `null`.

```quillo
type Lamp = { name: str, watts: int }
fn make() -> Lamp = Lamp({ name: "Desk", watts: 40 })
let desk = Lamp({ name: "Desk", watts: 40 })
route GET /a = `${desk.wats}`        // check: ok; the page: "null"
route GET /b = `${make().wats}`      // check: ok
```

`lamp.qwe: ok (1 route, 0 functions, 0 components)`, and `curl 127.0.0.1:8239` gives `Desk uses null watts` (the video shows both). Perhaps because `Lamp(v)` can give an error value (`err("Lamp: `watts` is missing")`), so its result is typed as anything; and `-> Lamp` on a fn is not used for what its callers read.

| Impact | |
|---|---|
| **Functionality** | A misspelled field reads `null`; arithmetic on it is an error at run time, text shows "null" |
| **User** | The visitor sees "null" in the page, or a 500 |
| **Requests** | No change |
| **Security** | No change (a check on a misspelled field, such as `user.is_admn`, is always false, which fails closed) |

**Suggestion:** type `Lamp({...})` as `Lamp | error` and a fn's `-> Lamp` as `Lamp`, and check field reads on them as on a typed parameter (an error value's fields are null, so the record's fields are the ones to check).

### Q-1371. An attribute's `{` left open after a lambda's block is reported as the element's text

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

One `}` left out of `onclick={() => { n = add_like() }}` (the lambda's block closes, the attribute's brace does not) is not reported as a missing `}`. The `>` that ends the tag is read as "greater than", so the attribute's expression goes on into the button's text, and the error names that text:

```quillo
component Like() {
    state n = 0
    <button onclick={() => { n += 1 }>♥ {n}</button>
}
route GET / = <Like/>
```

`main.qwe:3: error: unexpected character `♥``, followed by the Q-008 note (`note: this is quillo 0.8.10 ...; if the code was written for a newer Quillo, that would explain it`). With `+ {n}` as the text it is `expected an expression, found `+``. Neither points at the brace, and the note sends the reader to the version.

| Impact | |
|---|---|
| **Functionality** | No change (the program does not run) |
| **User** | A developer looks for something wrong with ♥ or with the version, not for a brace |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** when an attribute's `{ ... }` expression meets a `>` that is followed by text and a closing tag of the element, say the attribute's `{` (with its line and column) is not closed; and leave the version note off when the unexpected character is one no Quillo accepts in code (such as ♥).

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `db.open("app.db)` to show a lexer error | An unterminated string seemed the plainest lexer mistake | The next line's quote closed it: a parser error (`expected `)` after arguments, found `migrations``) | 1 check |
| Dropping a `}` from the handler to show a parser error | A missing brace seemed the plainest parser mistake | Q-1371's message; `</buton>` was used instead | 1 check |
| `lamp.wats` on a `Lamp(...)` value to show a checker error | The guide says check finds misspelled fields | Q-1370; `likes("all")` (an argument count) was used, and the miss went in the video as an honest one | 3 checks |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A curly quote in code | `unexpected character `“`, a curly quote: write `"`` |
| `</buton>` for `</button>` | `` `</buton>` does not close `<button>` from line 15: expected `</button>` `` |
| `likes("all")` for `likes()` | `` `likes` takes 0 arguments, got 1 ``, from `quillo check` and from `quillo run` (which refuses to start) |
| `db.migrate("migration")`, no such folder | check ok; `quillo run` stops before listening: `db.migrate: cannot read ./migration: there is no such folder (git keeps no empty folder: ...)` |
| `quillo docs main.qwe` | each server fn's contract, effects and `POST /_quillo/rpc/<name>` call |
| A save under `--watch` with the page open | `reloaded main.qwe (1 route)`; the page updated in place and kept the component's state (♥ 3) |
