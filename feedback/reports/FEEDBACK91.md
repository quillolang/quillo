# Feedback from building a live shopping list in Quillo, next to Datastar 1.0.5

| | |
|---|---|
| App | `~/dev/quillo-videos/videos/249-datastar/app/`: a shared shopping list (add, tick off, hide done, live in every tab), one `main.qwe` of 41 lines and a migration; the same app in Datastar 1.0.5 + Go in `compare/datastar/` |
| Quillo | quillo 0.8.11 (3602f18, 2026-11-25) |
| Model | Claude Opus 5.5 (httpdash-59, the HttpDash tab), for video 249, Quillo vs Datastar |
| Tested with | `quillo check`, Chromium through Playwright: `compare/measure.mjs` (two tabs, DevTools byte counts, 20 adds, two restarts) and `compare/mistakes.mjs` |

What worked well: the app is 41 lines against 159 of Go; `quillo check` caught each typo the Datastar build let through (a misspelled state, a misspelled server fn, a map passed for a `str`), with the right name suggested; names with markup were escaped; typing in one tab while another added an item kept the text and the focus; Hide done (a filter on state) sent no request.

## Seen again

None.

## New issues

### Q-1410. After the server is down for 20 s, a page's `live()` can take 12-13 s to catch up once the server is back

| Kind | Detected by | Attempts |
|---|---|---|
| perf | browser (`compare/measure.mjs`) | 2 runs |

The server was stopped, started again after 3 s or 20 s, and an item was added from a fresh tab; the script timed when the two pages already open showed it, with no reload. Quillo's pages always caught up, but after a 20 s outage they took 11.9 s and 13.2 s from the moment the server was back (earlier runs: 3.8 s and 11.3 s; 11.1 s and 11.5 s). Datastar's `@get('/updates')` stream, with its default retry (1 s, doubling, at most 30 s), caught up in 3.5 s (an earlier run: 7.5 s). After a 3 s outage Quillo was 0.3 s and 1.1 s (earlier: 4.7 s and 0.01 s).

Repro: `node videos/249-datastar/compare/measure.mjs quillo` in `~/dev/quillo-videos` (its `restart` entries in `compare/runs/quillo.json`).

| Impact | |
|---|---|
| **Functionality** | No change: every page catches up by itself |
| **User** | After a deploy or a crash of ~20 s, open pages show stale data for up to ~13 s more |
| **Requests** | None extra; the wait is the reconnect's back-off |
| **Security** | No change |

**Suggestion:** cap the live reconnect's back-off lower (a few seconds), or retry at once when the page regains the network or focus; say the schedule in the guide's Live data section.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `<style>body { font: 18px system-ui }</style>` in a route's markup | HTML habit | `expected }, found :`; the guide's "From HTML" says braces need changing: `<style>{css`...`}</style>` worked | 1 check |

## Limitations

- The page's JavaScript: 175,499 B (50,614 B gzipped) for this one component, against Datastar's 35,673 B (14,130 B gzipped). Not a bug; noted for the size work.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A checkbox with its own `onchange` and `checked={i.done == 1}` | stays ticked from the click on; no flicker while the live change comes back |
| An empty add through a `server fn` returning `err(...)` | the message shows; the input is kept |
| `<b>Bold</b> beans` as an item | shown as typed, no element made |
| A new item in another tab, adds a second apart (median of 20) | 37 ms, Datastar 38 ms: a tie. Adds back to back gave 125 ms, which is `LIVE_GATHER` doing what the guide's Live data section says (changes within ~0.1 s go as one refresh), not a slow path; an earlier draft of this report filed it as Q-1411 and was wrong |
