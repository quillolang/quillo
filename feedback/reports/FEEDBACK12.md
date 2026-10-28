# Feedback from building a real-time multiplayer game in Quillo

| | |
|---|---|
| App | [arena/](../../app/arena), "Quillo Arena": rooms where players steer a dot with the pointer and take orbs, a world the server moves 20 times a second, chat, rounds whose winners go to SQLite and a live leaderboard, spectators over Server-Sent Events, and bots; the game is a service of its own (`game.qwe`) beside the app's pages (`main.qwe`). 394 lines of Quillo in 2 files |
| Quillo | 0.5.0 (2cd134f); main at 6887b0b, whose code is the same |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; probes of each risky part first; headless Chrome and headless Edge (with QUIC on and off); [tools/arena-sim.mjs](../tools/arena-sim.mjs): 10 browser players and 0 to 400 bots in a room |

What worked well: the parts a game needs most. A channel carried 20 snapshots a second to
every player as WebTransport datagrams, and each of 10 players got all 20; a room of 400
bots and 10 players kept 20 ticks a second with a step of 12 ms and gaps of 50 to 55 ms
(below). What players send reached the world in order, one message at a time, with
`--instances 2` too. A page that could not use WebTransport (Edge with QUIC off) went on a
WebSocket by itself and got every update. `reliable: true` kept chat and rosters in
order beside lossy snapshots. Spectators followed a room's scores with `events` and
`listen`, five lines on each side. `quillo check` found four of my mistakes before they
ran (a link inside a link, `int` given two arguments, two live loaders of another
service's data that nothing would wake).

What it cost: there is no timer on the server (Q-080), so the loop is a job that never
ends, and such a job holds shutdown for 10 s and ends as "a bug in Quillo" (Q-081), and
survives `--watch` reloads with the old code while the new loop never starts (Q-082): that
one left the game frozen for the person watching it, after an ordinary edit. As a
service in the app's process, the game's jobs never ran (Q-083); split apart, its channels
cannot be reached (Q-087). And the guide's own app made only of services does not serve
(Q-088).

## Fixed, on branch feedback12-fixes

Each fix is commented in the code with its issue and the branch. All repros run again
(`node feedback/run.mjs`): 69 fixed, 2 open (Q-084 and Q-087, below); `cargo test` passes.

| ID | The fix |
|---|---|
| Q-080 | `every(ms, f)` at the top of a program is a tick on the server: `f` runs every `ms` milliseconds, on multiples of `ms` counted from 1970, one at a time (a tick still running skips the next), named after its function (`{ name }`, else `every-<line>`; two alike are an error). Each tick has a thread of its own that asks `POST /_quillo/tick/<name>`, which is neither logged nor counted nor recorded in `_quillo_jobs`. Arena's loop is now `every(TICK_MS, tick)`, and its forever job is gone (src/jobs.rs `every_at_top`, `start_ticks`; src/builtins.rs `every`; src/server.rs `run_tick`) |
| Q-081 | `sleep` waits in slices of 50 ms and ends the request or job with a 503 ("the server is stopping") once the server stops, so a forever job ends with it: the repro's server stops at once, without "a bug in Quillo". A job still running when it is due again is said once a run, not each second (src/builtins.rs `sleep`; src/jobs.rs) |
| Q-082 | A reload retires the version it replaces (its mounts and services too): a `sleep` in it ends ("the program was loaded again"), so the old loop stops and the new version's job starts at its next time (src/server.rs `retire`; src/interp.rs `retired`) |
| Q-083 | A service's `schedule` and `every` run with the app's when the service shares its process, as `service~job` (`POST /_quillo/job/game~end_round` in the log), each in the service's own interpreter. Split, the service's process runs them as before. Arena's `start_game` workaround is gone (src/server.rs `start_jobs`, `run_job`) |
| Q-085 | `quillo check` lets a plain `fn` set a field of a parameter of no declared type, as a page object a component hands it (`paint(el.getContext("2d"))`); a known map or value is still refused, and the server still refuses it at run time (src/check.rs, `Stmt::SetField`) |
| Q-086 | From the moment a channel's `join` is called for a page to the moment the page hears yes, what is broadcast on the channel, or sent to that page, is held for it (up to 256 messages) and goes to it right after the yes; dropped on a no. Arena's `hello` round trip is gone: the roster `join` broadcasts is the first thing a page hears (src/wt.rs `joining`, `hold`, `joined`) |
| Q-088 | A program serves when it has routes, server functions, mounted or remote services, channels, UDP listeners, jobs or ticks, not only routes and server functions: the repro's app of two services serves both (src/main.rs) |

Not fixed:

- **Q-084** stays open, documented: the guide's Live data section says a global is one
  topic, and to keep what changes often in a global of its own. Following a field of a
  global is a change to how live data records what it reads, left for its own round.
- **Q-087** stays open: `--split` now warns, at start, of each service whose code uses
  channels (``quillo: warning: service s (s.qwe) uses channels, and with --split its pages
  cannot reach them ...``), and the guide's Services section says so. The bridge (pages'
  messages forwarded to the service's process, its broadcasts back to the app's) is not
  built.

The user guide has the rest: Ticks (`every` on the server), a forever job and `sleep` at
stop and reload, a service's jobs, what `join` broadcasts, and a helper setting a page
object's fields.

A second game, written afterwards on the fixed build, used them as a newcomer would:
Quillo Snake (`app/snake/main.qwe`), a multiplayer snake that grows by two cells a bite,
with bots. Its loop is a top-level `every(110, tick)` (9 ticks a second; over 4,000
ticks, 1 to 3 ms a step and 12 ms at worst), and its `join` sends the page its id and broadcasts the roster, both
heard by the page joining. Headless Chrome played it with the keyboard: no errors on the
page.

## Seen again

None of the known issues; the new ones are below.

## New issues

### Q-083. A service's `schedule` never runs while the service shares the app's process

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the game standing still, then a probe | 2 |

The game is a service (`service /game = "game.qwe"`), run in the app's process as the
default is. Its `schedule("1s", game_loop)` never ran: `/game/stats` said 0 ticks, with no
line in the log. Run alone (`quillo run game.qwe`) the same file's job runs. The scheduler
asks only for the app's own jobs (`it.site.jobs` in `server::start_jobs`), and a service's
are in the service's interpreter. Its `channel.on` handlers, on the other hand, are heard
in one process.

```quillo
// main.qwe
service /s = "s.qwe"
route GET / = "the app"

// s.qwe
let runs = 0
fn tick() { runs += 1 }
schedule("1s", tick)
route GET /runs {
    sleep(2500)
    { runs }                // {"runs":0} through the app; 2 or 3 alone
}
```

**Workaround in the app:** the app's own job calls a `server fn` of the service that runs the
loop (`schedule("1s", () => game.run_loop())`), for one process only.

**Suggestion:** schedule every loaded program's jobs: the app's and its services' in the
process, each run in its own interpreter (`/_quillo/job/<service>/<name>`).

### Q-082. Under --watch, a job still running at a reload goes on with the old code, and the new one never starts

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the person watching the game: a blank room after an edit | 1 |

A reload makes a new program, but the old program's job is still inside its loop: it
keeps running the old code against the old variables, and the scheduler skips the new
version's job by its name ("job game_loop is still running; skipped this time", each
second, for ever). The page talks to the new program, whose world never moves.

```quillo
let n = 0
fn game_loop() {
    while true {
        sleep(100)
        n += 1
    }
}
schedule("1s", game_loop)
route GET / = { n, version: 1 }   // edit to version: 2 under --watch: n stays 0
```

**Suggestion:** a job is the program version's: at a reload, the old version's running jobs
are told to stop (their next `sleep` or wait ends them, as a stream's `send` does when its
client leaves), and the new version's start. Better still, a game's loop should not be a
job at all (Q-080).

### Q-087. Split apart, a service's channels are out of the pages' reach

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probing the game as a service with `--split` | 1 |

Pages connect to the process that served them, the app. With `--split`, the service's
`channel.on` handlers are in another process: what pages send goes nowhere (a channel no
handler names takes messages and drops them), and what the service broadcasts reaches no
page. Nothing says so; in one process the same program works.

```quillo
// main.qwe, run with --split
service /s = "s.qwe"
component Talk() {
    state got = 0
    let ch = channel.open("svc", (m) => { got += 1 })
    if true { every(100, () => { ch.send({ x: 1 }) }) }
    <p>{got}</p>                // 0, and s.qwe's `heard` stays 0
}
route GET / = <html><body><Talk/></body></html>

// s.qwe
let heard = 0
fn tick() { channel.broadcast("svc", { at: now() }) }
schedule("1s", tick)
channel.on("svc", (msg, peer) => { heard += 1 })
route GET /n = { heard }
```

**Suggestion:** the app passes channel traffic on to the service that declares the channel,
as it passes routes under a service's prefix: a page's message to the service
(`POST /_quillo/channel/<name>`, already the internal form), and the service's broadcasts
back to the app, which sends them to its pages. Until then, say so: `--split` should warn
that a service handles channels its pages cannot reach.

### Q-086. What `join` broadcasts does not reach the page that is joining

| Kind | Detected by | Attempts |
|---|---|---|
| silent | names missing on the canvas ("?"), then a probe | 2 |

A room sends its roster as someone enters, from `join`. Everyone else got it; the person
entering did not, so their own screen had no names. The page becomes a member only after
`join` answers yes, so whatever `join` sends is gone before it can hear it. The guide does
not say.

```quillo
channel.on("room", (msg, peer) => null, { join: (peer) => {
    channel.broadcast("room", { hi: peer.id }, { reliable: true })
    true
} })

component P() {
    state heard = 0
    let ch = channel.open("room", (m) => { heard += 1 })
    <p>{heard}</p>              // 0: the greeting went out before this page was in
}
route GET / = <html><body><P/></body></html>
```

**Workaround in the app:** the page sends `{ hello: 1 }` once joined, and the handler answers
it alone with `channel.send(name, peer, roster)`.

**Suggestion:** a page `join` lets in gets what was sent on the channel while `join` ran.

### Q-088. A program made only of the services it mounts does not serve

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a repro that printed nothing | 1 |

The guide's own example under Services ends with "app.qwe: the app, made of both", two
`service /prefix = "..."` lines and nothing else. `quillo run app.qwe` treats a program
without routes of its own as a script: it runs it, and exits 0, without a word, though
both services have routes under their prefixes.

```quillo
// main.qwe
service /shop = "shop.qwe"     // quillo run main.qwe: exits 0 at once
// shop.qwe
route GET / = "the shop"
```

**Suggestion:** a program serves when it or a service it mounts has routes (or channels,
UDP listeners or jobs).

### Q-080. `every(ms, f)` at the top of a program does nothing on the server, and nothing runs more often than once a second

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe before building | 1 |

A game's server needs a loop: move everything, send the world, 20 or 60 times a second.
`every(50, step)` at the top of a program passes `quillo check` and never runs (it is a
browser function, which does nothing on the server). `schedule` takes whole seconds, its
clock is looked at once a second, and a new job waits for its next time, so it cannot be
the loop either. What is left is a job that never ends, sleeping to the next tick, which
is how Arena runs, and which brings Q-081 and Q-082.

```quillo
let ticks = 0
every(50, () => { ticks += 1 })
route GET / {
    sleep(1200)
    { ticks }                 // {"ticks":0}
}
```

**Suggestion:** `every(ms, f)` at the top of a program runs `f` on the server every `ms`
milliseconds, on the clock (so it does not drift), as a job runs (one at a time, skipped
while the last is still running), from when the program loads until it is reloaded or
stops. It is the browser's `every`, so a game's client and server loops read alike.

### Q-081. A job that never ends holds a stopping server for 10 s, and ends as "a bug in Quillo"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | a probe of the workaround | 1 |

A stopping server waits for running requests, and a job is one, so `SIGTERM` waits the
full drain (10 s), then the job ends as `quillo: internal error (a bug in Quillo) while
handling POST /_quillo/job/game_loop`. While it runs, the log says "job game_loop is still
running; skipped this time" every second.

```quillo
fn game_loop() {
    while true { sleep(50) }
}
schedule("1s", game_loop)
route GET / = "ok"            // kill -TERM: 10 s, then the internal error
```

**Suggestion:** a stopping server ends a job at its next `sleep` or wait (as a stream's
`send` ends when the client leaves), without calling it a bug; and "still running" is said
once per run, not each second.

### Q-084. A global is one topic for live data, so a loader of one field runs on every write to any

| Kind | Detected by | Attempts |
|---|---|---|
| perf | a probe before building the lobby | 1 |

A scoreboard that reads `world.scores` ran again about 19 times a second for each open
page, because the loop assigns `world` (its `tick`) 20 times a second, and live data follows
the variable as a whole. What it reads never changed.

```quillo
let world = { tick: 0, scores: { ana: 0 } }
// a job writes world = { ...world, tick: world.tick + 1 } 20 times a second
server fn scores() = world.scores
component Board() {
    state s = live(scores)    // runs again 18.8 times a second, for each page
    <p>{json.stringify(s)}</p>
}
```

**Workaround in the app:** what pages follow (the rooms' counts) is a global of its own, and
the game publishes a topic when it changes.

**Suggestion:** a loader's run is not sent when its result did not change (it is not sent
today, but it runs); and the guide could say that a global is one topic, with this
workaround.

### Q-085. `quillo check` refuses a helper that sets a field of a page object it is given

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 2 |

The drawing was a plain `fn draw(ctx, m, names)` called from the component. `quillo check`
refuses `ctx.fillStyle = ...` there ("only the page's objects (an element, an event) have
fields that are set, in a component"), though `ctx` is the canvas's context, and with
`--no-check` the browser draws. The same lines inside the component pass.

```quillo
fn paint(ctx, colour: str) {
    ctx.fillStyle = colour    // error: cannot set `fillStyle`
    ctx.fillRect(0, 0, 10, 10)
}
component A() {
    <canvas width="10" height="10" @mount={(el) => { paint(el.getContext("2d"), "red") }}></canvas>
}
route GET / = <html><body><A/></body></html>
```

**Workaround in the app:** the drawing is a lambda inside the component.

**Suggestion:** in a function components call, a parameter that is not given a type may be a
page object: allow setting its fields there (and keep refusing it for a map's).

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `every(50, step)` at the top, for the server loop | it is the browser's clock, and the guide says nothing against it at the top | silently nothing (Q-080) | 1 probe |
| A forever job as the loop | the only timer left | Q-081, Q-082; Q-082 froze the game for the person watching | 1 restart, then `--no-watch` for the demo |
| The game as a service, its job in it | services are meant to be split later | the job never ran (Q-083), then split cut the channels (Q-087) | 3 probes |
| `fn draw(ctx, ...)` beside the component | sharing drawing code | refused by `quillo check` (Q-085) | 1 check |
| The roster sent from `join` | the obvious place | the joining page never saw it (Q-086) | 1 screenshot, 1 probe |
| `<a href="/room/x">... <a href="/watch/x">watch</a></a>` | a card that is a link, with a link in it | `quillo check`: a link inside a link | 1 check |
| `int(s, 16)` for a hue from an id | JavaScript's `parseInt(s, 16)` | `int` takes 1 argument | 1 check |
| `fn nearest(orbs, x: float, y: float)` | positions are numbers | an `int` is not a `float`: the loop failed each second (`random_int` starts bots on whole numbers) | 1 run |
| `first(for [i, x] in enumerate(xs) => f(x) ? i : null \| filter(...))` | a pipe after the `for` | the pipe belonged to the `for`'s body, so `first` got a list of lists | 1 run |
| `service game = "game.qwe"` without a prefix | the guide's first form | its `/stats` and `/bots` routes were not served; `service /game = ...` mounts them | 1 request |
| Live loaders of the service's counts, no topic | as for the app's own data | `quillo check` warned that they would never change; `publish`/`watch` added | 1 check |
| My simulation's `page.ev("expr")` | the tool's function, read too fast | it takes a function body: `return expr` | 1 run |

## Limitations

- **Messages are JSON.** A snapshot costs about 21.6 bytes a player; past about 1,100 bytes
  (here 30 players and 24 orbs) every snapshot goes on a stream, in order, instead of as a
  datagram: 100% of them from 50 players on. There is no binary message, and no way to
  send only what changed but by hand.
- **Channels stay on one server.** Broadcasts are not relayed between servers behind limer
  (as the guide says), so a room's players must reach the server that holds it.
- **A background tab** runs `every` once a second at most (the browser's rule), so a player
  whose tab is hidden sends no moves.
- **WebSocket connections are not in the request log** (`/_quillo/wt`, `/_quillo/live`):
  whether a page fell back from WebTransport cannot be read from the server's side.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| 10 players at 20 updates a second | each got 20.0 a second, at every load below |
| A room of 400 bots | 20 ticks a second, step 12 ms, gaps 50–55 ms, none late |
| `--instances 2`, moves from pages | all 149 moves of a page reached the one world (the job runs once a process) |
| Edge with QUIC off | WebTransport refused, the page went on a WebSocket and got every update |
| Messages from one page | reached `channel.on` in the order sent, one at a time |
| `reliable: true` beside datagrams | chat and rosters in order, snapshots dropped when late |
| Spectators | `events` and `listen`: scores twice a second, the stream ended when the page left |
| A service's `channel.on` in one process | heard pages' messages (only its jobs did not run: Q-083) |
| Hostile channel messages | a move outside the board is clamped; chat is cut to 200 characters; a page without a name is refused by `join` |

## Simulation, before the fixes

`node feedback/tools/arena-sim.mjs http://127.0.0.1:8087 10 0,20,50,100,200,400 6`, on an
Apple M5, the loop being Arena's forever job (the workaround for Q-080):

| bots | players | ticks/s | step ms | worst ms | gap p50 | gap p99 | gap max | late | snapshot B | on a stream | updates/s min | updates/s avg |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 10 | 20.0 | 0 | 2 | 50 | 56 | 57 | 0 | 522 | 0% | 20.0 | 20.0 |
| 20 | 10 | 20.2 | 1 | 5 | 50 | 55 | 55 | 0 | 934 | 0% | 20.2 | 20.2 |
| 50 | 10 | 20.0 | 2 | 6 | 50 | 55 | 55 | 0 | 1575 | 100% | 20.0 | 20.0 |
| 100 | 10 | 20.0 | 3 | 7 | 50 | 55 | 55 | 0 | 2618 | 100% | 20.0 | 20.0 |
| 200 | 10 | 20.0 | 7 | 11 | 50 | 55 | 56 | 0 | 4801 | 100% | 20.0 | 20.0 |
| 400 | 10 | 20.0 | 12 | 13 | 50 | 55 | 55 | 0 | 9179 | 100% | 20.0 | 20.0 |

## Simulation, after the fixes

The same run on branch feedback12-fixes, the loop being a top-level `every(50, tick)`:

| bots | players | ticks/s | step ms | worst ms | gap p50 | gap p99 | gap max | late | snapshot B | on a stream | updates/s min | updates/s avg |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 10 | 20.0 | 0 | 1 | 50 | 59 | 59 | 0 | 525 | 0% | 20.0 | 20.0 |
| 20 | 10 | 20.0 | 4 | 5 | 50 | 60 | 63 | 0 | 948 | 0% | 20.0 | 20.0 |
| 50 | 10 | 20.0 | 2 | 6 | 50 | 59 | 60 | 0 | 1580 | 100% | 20.0 | 20.0 |
| 100 | 10 | 20.0 | 2 | 6 | 51 | 59 | 60 | 0 | 2621 | 100% | 20.0 | 20.0 |
| 200 | 10 | 20.0 | 8 | 11 | 50 | 59 | 59 | 0 | 4806 | 100% | 20.0 | 20.0 |
| 400 | 10 | 19.8 | 10 | 55 | 50 | 58 | 93 | 0 | 9165 | 100% | 19.8 | 19.8 |

The rate holds at 20 ticks a second to 400 bots, and every player still gets each
update. The gaps spread a little wider than the forever job's (p99 59 ms against 55 ms),
since each tick is a request over the loopback rather than a `sleep` in one loop; one
gap of 93 ms at 400 bots skipped one tick (19.8 a second). `late` no longer applies: a
tick never runs behind, it skips.
