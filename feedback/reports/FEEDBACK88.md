# Feedback from making a single-page-app course (Fernhill Trails, lessons 1-3)

| | |
|---|---|
| App | Fernhill Trails, the trail guide built across the "Quillo Single-Page Apps" course in `~/dev/quillo-videos/spa/` (lesson 3 is `spa/03-page-data`): `App` reads `url().path`, `route GET /*rest` renders every address, and since lesson 3 each page loads its own data with a `server fn` in a `state` initializer (`state trail = load_trail(id)`) |
| Quillo | `quillo 0.8.10 (61997ab, 2026-11-22)`, the binary the course pins. Both repros were run on it |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; Chromium through Playwright, with a mouse (hover, then click) and with `hasTouch` taps; `node feedback/run.mjs Q-1380 Q-1381` |

Numbers Q-1380 and Q-1381, from the block Q-1380 to Q-1389.

## New issues

### Q-1380. A page whose loading state reaches a typed view as null stays blank for good after a tap, and check says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser (making a video) | 3 |

Lesson 3's trail page, before the lesson's fix:

```quillo
component TrailPage(id: int) {
    state trail = load_trail(id)            // server fn load_trail(id: int) -> map?
    <main>
        <h1>{trail.name}</h1>
        <Level level={trail.level}/>        // view Level(level: str)
        ...
    </main>
}
```

Opened at `/trails/5`, the server runs `load_trail` before sending the page, so it is whole. Clicked with a mouse, it is whole too: the pointer resting on the link prefetches `/trails/5` (`GET /trails/5 -> 200 prefetch`) and the click uses it. Tapped on a touch screen (Chromium with `hasTouch`, `tap()`), the prefetch starts with the tap and comes too late; the page is drawn in the browser with `trail` null, `Level` gets null, and the browser's console says `quillo: ui/trails.qwe:10: argument `level` of `Level` must be str, got null null`. `POST /_quillo/rpc/load_trail -> 200` answers in about a millisecond, but the page stays blank (no `<main>` at all) for as long as it is open: the answer never draws it. A reload is whole again.

So the mistake the guide warns of ("show something for `null`") is invisible on a laptop and a blank page on a phone, and nothing outside the browser's console says so. `quillo check` says `ok` for `trail.level` passed to a `str` parameter where `trail` is a `map?` state.

Two things would each have caught it: check warning of a `map?` state's field going to a typed parameter (or of any field of a server-fn state read without a null check, as it knows that state starts null in the browser); or the component drawing again when the state changes after a failed draw, so the page shows once the answer comes. The repro uses a script's `click()`, which is a tap (no hover).

Repro: `repros/Q-1380-null-state-render-stays-blank.qwe` (open on 61997ab).

| Functionality | User | Requests | Security |
|---|---|---|---|
| a page is blank for good | anyone on a touch screen, after a tap | none failed: the RPC answers 200 | none |

**Suggestion:** a check warning for a field of a server-fn `state` read without a null test (at least when it goes to a typed parameter); and in the browser, drawing a component again on its next state change after a draw failed, instead of leaving it empty.

### Q-1381. `redirect()` in a server fn called from a component goes nowhere, and check says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probe (planning a lesson) | 2 |

A route may end with `redirect("/hikes/7")`, and it is natural to write a `server fn log_hike(...)` the same way. Called from a component's handler, the call answers `303`, `fetch` follows it (`POST /_quillo/rpc/log_hike -> 303`, then `GET /hikes/7 -> 200` in the log), the call gives `err("internal error")`, and the page stays at `/`. Nothing in the server log or the page says that `redirect` means nothing to a call; `quillo check` says ok. First seen on 0.8.8 (2026-11-21) while planning lesson 6 of the course, and again on 0.8.10.

Repro: `repros/Q-1381-redirect-in-server-fn.qwe` (open on 61997ab).

| Functionality | User | Requests | Security |
|---|---|---|---|
| a save "does nothing" | the page stays put; the call's value is an opaque error | 303, then a GET nobody reads | none |

**Suggestion:** check warns of `redirect` in a `server fn` that a component calls (return the id, and `navigate`), or the runtime follows a `redirect` from a call with `navigate`; at least the error value could say "a server fn can't redirect; return a value and navigate".

## Where the AI struggled

- Q-1380 first looked like a plain "null renders blank fields" mistake, as it did on 0.8.8 with untyped fields. A mouse click worked every time; only a tap reproduced it, which took three probes to see.

## Edge cases that behave well

- `url().path` and a `match` on its pieces, links that change the page with no load, Back and Forward, and `route GET /*rest` with `respond(404, page)` for unknown addresses (lessons 1-2).
- `str(id) != piece` keeps one written form for a number piece; a typed route piece (`:id(int)`) already refuses `05` and `+5`, as documented.
- With the fix (`trail == null ? <Waiting/> : is_err(trail) ? <NotFound/> : ...`), a tap on a slowed network (1.5 s a request) shows the placeholder, then the trail.
