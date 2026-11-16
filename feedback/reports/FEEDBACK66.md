# Feedback from building chatrooms in qwertime: a channel callback that cannot answer on its channel, breakpoints that never stop, a warning that is wrong twice, and half a megabyte for each page on a channel

| | |
|---|---|
| App | qwertime.com's Rooms, outside this repo (`~/dev/qwertime.com`, commit 977505f, live since 2026-11-14). Chats of 1 to 8 hours that keep nothing in the database: a room lives in the two app servers' memory, and what is said lives only in the pages of those in it. It has a host who lets people in or turns them away, an invite link, a name and a colour for each person, and the host's page asks the others for what was said when it comes back. `lib/rooms.qwe` (540 lines) covers the rules, the channel and the two servers telling each other. `ui/rooms.qwe` (326 lines) holds the pages and `Room`, the site's first component, and `main.qwe` gained 88 lines |
| Quillo | Found on `quillo 0.8.1 (89a91c2, 2026-11-09)`, which qwertime runs. Each issue was checked again on this repo's `target/release/quillo`, `quillo 0.8.1 (80e5ea3, 2026-11-14)` on branch `feedback65`, on 2026-11-14. That build was made after the branch's last commit, so it may include the branch's uncommitted changes to `src/httpd.rs` and `src/interp.rs` |
| Model | Claude (Opus 5.5, `claude-opus-5-5`), which built the feature and wrote this report |
| Tested with | `quillo check`; Quillo unit tests (18 checks); Node HTTP tests (51 checks for rooms, 543 in the whole suite, 99.5% line coverage). The tests open the channel as Quillo's runtime does (`POST /_quillo/wt-ticket`, a WebSocket to `/_quillo/wt`, frames `["j",…]`, `["m",…]`) and play a second app server. Also headless Chrome over the DevTools protocol at desktop and phone sizes, in light and dark, and production behind limer |

No Quillo source was changed, and neither were `feedback/INDEX.md` or `feedback/repros/`. Each repro is in its own section. Numbers start at Q-950, after FEEDBACK65.md's Q-949.

What worked well: channels were the right tool, and their wire protocol is simple enough to drive from a Node test with a ticket and a WebSocket. `join` can `channel.send` to the page that is joining, which is how a page re-opens its room after a reconnect. `channel.send` by peer id works from an HTTP route as well as from the channel's handler. `crypto.hmac` gives the same hex as Node's `createHmac("sha256")`, so the test could sign as a second server would, and `request.body` gives the raw text to check a signature against. The component resumed and was not made again (`remade: 0` in Chrome). A `state` set from `js(...)` was read once, in the browser, and `date.from_ms` gave the same `HH:MM` on the server and in Chrome. `quillo check` caught a `key` parameter, a name defined in two files, and every missing effect, each on the first try.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-015 | `view RoomDoor(r: map, …, key: str, …)` for a room's invite key. `quillo check` refused it, as fixed, with the reason and a name to use instead; it became `invite_key` | 1 |

## New issues

### Q-950. A component's channel callback that answers on the channel it opened passes `quillo check`, and the server does not start

| Kind | Detected by | Attempts |
|---|---|---|
| loud (at start, not in `quillo check`) | the test suite's server not answering | 1 full test run, then a rewrite |

The natural way to answer the server on a channel is from the callback: `let room = channel.open(name, (m) => { … room.send(…) })`. The server sends "hello" from `join`, which reaches the page that is joining, so that after each reconnect the page says again which room it shows. The page answers from the callback. `quillo check` passes this. `quillo run` then compiles the component for the browser as it starts, fails, and exits 1, so nothing is served. `state room = channel.open(…)` is refused by `quillo check` instead (`unknown name room`), so neither form works.

```quillo
channel.on("room", (msg, peer) => null, { join: (peer) => {
    channel.send("room", peer, { t: "hello" })
    true
} })

component Room() {
    state hellos = 0
    let room = channel.open("room", (m) => {
        // Told it is connected (again, after a drop): say which room this page shows.
        if m.t == "hello" { room.send({ op: "open" }) }
        hellos += 1
    })
    <p>{hellos}</p>
}

route GET / = <html><body><Room/></body></html>
```

On both builds:

```
$ quillo check q950.qwe
q950.qwe: ok (1 route, 0 functions, 1 component)
$ quillo run q950.qwe --port 18990
quillo: q950.qwe:10: <Room> (compiled for the browser): unknown name `room`
[exit 1]
```

Expected: either the callback may use the channel it is the callback of (the callback runs only after `channel.open` has returned, and the same code in JavaScript, `const room = open(…, (m) => room.send(…))`, is fine), or `quillo check` reports what the browser compile reports. The two should never disagree.

The workaround qwertime uses: the callback only sets state (`hellos += 1`, or the host's request as a map with a counter), and a statement in the component's body that reads that state sends. Such a statement runs again whenever the state changes:

```quillo
    if hellos > 0 { room.send({ op: "open", room: id }, { reliable: true }) }
```

What it cost: one full run of the test suite (about 5 minutes) to find out, then a rewrite of the component's three sends (open, the host's recall, and the answers to it).

| Impact | |
|---|---|
| **Functionality** | A program that passes `quillo check` does not run at all |
| **User** | The developer sees `ok` from `quillo check`, then the server exits at start. The message names the right line. Visitors get nothing from that server |
| **Requests** | None are served. With `--register` the server never joins limer's pool. A deploy that relies on `quillo check` alone before restarting would restart its first server into this and leave it down. qwertime's deploy checks on the server and then restarts each server in turn; it is saved only because its tests start the server first |
| **Security** | No change |

**Suggestion:** let the browser compile see a `let`'s own name inside a lambda in its initializer, as the server does, or make `quillo check` compile components for the browser too, so it fails where `run` fails. The guide's Channels section could show the server answering a page and the page answering back, since that is where a reader reaches for `room.send` in the callback.

### Q-951. The debugger marks a breakpoint in a component's handler as verified, but that code runs only in the browser and never stops there

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a coverage tool built on `quillo dap` (qwertime's `tests/coverage.mjs`) counting the component's lines as never run | 1 |

`quillo dap` answers `setBreakpoints` with `verified: true` for a line inside an `onclick` handler. That code runs in the browser, so the server's debugger can never stop there: the breakpoint shows as set and is never hit. qwertime's coverage tool puts a logpoint on every line the debugger verifies. It counted 35 lines of `Room`'s callback and handlers as statements the tests never ran. With them, the whole program stood at 98.5%, under its 99% deploy gate. Without them, and with unit tests for the component's helpers, it is at 99.5%.

```quillo
component Counter() {
    state n = 0
    <div>
        <p>{n}</p>
        <button onclick={() => {
            n += 1
        }}>+</button>
    </div>
}

route GET / = <html><body><Counter/></body></html>
```

A minimal DAP client (Node) launches it under `quillo dap` and asks for breakpoints on line 2 (the state, which the server runs as it renders the page) and line 6 (in the handler, the browser's):

```js
import { spawn } from "node:child_process";
import { resolve } from "node:path";
const dap = spawn(process.argv[2], ["dap"], { stdio: ["pipe", "pipe", "inherit"] });
let buf = Buffer.alloc(0), seq = 1, onInit;
const waiting = new Map(), initialized = new Promise((ok) => (onInit = ok));
const send = (command, args = {}) => new Promise((ok) => {
  const s = seq++; waiting.set(s, ok);
  const j = JSON.stringify({ seq: s, type: "request", command, arguments: args });
  dap.stdin.write(`Content-Length: ${Buffer.byteLength(j)}\r\n\r\n${j}`);
});
dap.stdout.on("data", (d) => {
  buf = Buffer.concat([buf, d]);
  for (;;) {
    const h = buf.indexOf("\r\n\r\n"); if (h < 0) break;
    const n = +/Content-Length: (\d+)/i.exec(buf.subarray(0, h).toString())[1];
    if (buf.length < h + 4 + n) break;
    const m = JSON.parse(buf.subarray(h + 4, h + 4 + n).toString()); buf = buf.subarray(h + 4 + n);
    if (m.type === "response") { waiting.get(m.request_seq)?.(m); waiting.delete(m.request_seq); }
    else if (m.event === "initialized") onInit();
  }
});
await send("initialize", { adapterID: "quillo", linesStartAt1: true, columnsStartAt1: true });
send("launch", { program: resolve("main.qwe"), args: ["--port", "0"], stopOnEntry: false });
await initialized;
const r = await send("setBreakpoints", { source: { path: resolve("main.qwe") }, breakpoints: [{ line: 2 }, { line: 6 }] });
for (const b of r.body.breakpoints) console.log(`line ${b.line}: ${b.verified ? "verified" : "not verified"}`);
dap.kill("SIGKILL"); process.exit(0);
```

On both builds (`node dap.mjs "$(which quillo)"`):

```
line 2: verified
line 6: verified
```

Expected: line 6 is not verified, with a message saying it runs in the browser.

qwertime's workaround: its coverage tool leaves out every line from `component Name(` to the component's closing brace. The component's pure helpers moved to the top of the file and got unit tests.

| Impact | |
|---|---|
| **Functionality** | No change to the program |
| **User** | In an editor, the developer gets a solid breakpoint that never stops. A coverage tool on the DAP counts browser-only lines as never run and cannot tell them apart from untested server code |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** answer `verified: false` for lines that run only in the browser (handlers, `channel.open` callbacks, `every` in a component), with a `message` such as "runs in the browser: set it in the browser's devtools (under `--watch`, each component is a file of its own)". A tool can then skip them.

### Q-952. A `js(...)` call in a channel callback makes `quillo check` warn that the channel is "a value from the page shown in markup", which is untrue, with a fix that cannot be written

| Kind | Detected by | Attempts |
|---|---|---|
| loud (a misleading warning) | `quillo check`, then about 25 checks of cut-down copies to find the cause | about 27 |

A channel callback that calls `js(...)` (here, to log; in qwertime, to scroll the newest line into sight), together with a handler inside a conditional part of the markup that sends on the channel, gets a warning. The warning says the browser makes the component again on each load, and to write `state room = ...`. Neither is so:

```quillo
channel.on("room", (msg, peer) => null)

component Room() {
    state n = 0
    let room = channel.open("room", (m) => {
        n += 1
        js("console.log", n)
    })
    <div>{n >= 0 && <button onclick={() => { room.send({ op: "x" }) }}>Send</button>}</div>
}

route GET / = <html><body><Room/></body></html>
```

```
$ quillo check q951.qwe
q951.qwe:9: warning: `room` is a value from the page shown in markup: the server has null for it, so the browser makes <Room> again on each load. Write `state room = ...`, which is set once the component is on the page
q951.qwe: ok (1 route, 0 functions, 1 component), 1 warning
```

- Without the `js(...)` line there is no warning. Without the `n >= 0 &&` (the button always shown) there is none either.
- In headless Chrome (this repo's `feedback/tools`, browser mode), the page is not made again: `"resumed": 1, "remade": 0`, with no mismatches and no errors.
- The advice cannot be followed when the callback uses the channel: `state room = channel.open(…)` is `unknown name room` (Q-950). When it does not, the advice is not needed.

What it cost: about 25 checks of cut-down copies of qwertime's component before the trigger turned out to be `js(...)` in the callback. Two guesses came first: a handler written without braces (`() => room.send(…)`), and the callback using `room` itself. The fix was to take `js` out of the callback. The scrolling became a body statement (`js("qtRooms.down", len(lines), status)`), and the browser's time zone a `state zone = js("qtRooms.zone") ?? "UTC"`, read once.

| Impact | |
|---|---|
| **Functionality** | No change: the page resumes |
| **User** | The developer is told something false, and the remedy offered either does not compile or is not needed |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** what `channel.open` returns is a handle the page makes. It is shown in no markup, only called in handlers, so whatever marks values as "from the page" should not reach it through the callback's body. If the warning stays for some cases, it could name the expression it saw shown.

### Q-953. The guide's list of components that start by themselves leaves out `channel.open`

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading, then a probe in headless Chrome | 1 |

USERGUIDE.md line 1740 says a component loads as soon as the page is idle when it "runs by itself (`every`, `live`, `listen`, `js`, `js_new`, `@mount` …)". Otherwise it waits for someone to reach for it. `channel.open` is not in the list, so a reader would expect a chat that only listens to wait for a touch. It does not wait. On both builds, a component with nothing but `channel.open` started as the page loaded and, untouched, showed 13 of the server's broadcasts (one every 200 ms) within about 2.5 s:

```quillo
let n = 0
fn tick() with net { n += 1; let _ = channel.broadcast("news", { n }) }
every(200, tick)

component News() {
    state heard = 0
    let ch = channel.open("news", (m) => { heard += 1 })
    <p id="h">{heard}</p>
}

route GET / = <html><body><News/></body></html>
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A reader may add an `every` or a `js` only to make the component start |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** add `channel.open` (and media's, if it does the same) to the list.

### Q-954. Each page on a channel costs the server about half a megabyte, and most of it stays after the page closes

| Kind | Detected by | Attempts |
|---|---|---|
| perf (memory) | a load test of qwertime's rooms (1,000 pages in one room), then a program with nothing but a channel | 2 |

A program with one channel and nothing else, and 1,000 pages joining it, as Quillo's runtime joins (`POST /_quillo/wt-ticket`, a WebSocket to `/_quillo/wt`, `["j","c"]`), measured with macOS `footprint` (which counts what the memory compressor hides from `ps`):

```quillo
channel.on("c", (msg, peer) => null, { join: (peer) => true })
route GET / = "ok"
```

```js
// node pages.mjs <base> <n> <pid>: n pages join "c"; the server's footprint before, with them open, and 10 s after they close; three times.
import { execSync } from "node:child_process";
const [BASE, N, PID] = [process.argv[2], +process.argv[3], process.argv[4]];
const fp = () => (execSync(`footprint -p ${PID}`).toString().match(/phys_footprint:\s*([\d.]+ \w+)/) || [])[1];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
console.log("before:", fp());
for (let round = 1; round <= 3; round++) {
  const pages = [];
  for (let i = 0; i < N; i += 50) pages.push(...await Promise.all(Array.from({ length: Math.min(50, N - i) }, async () => {
    const { ticket } = await (await fetch(BASE + "/_quillo/wt-ticket", { method: "POST", headers: { origin: BASE } })).json();
    const ws = new WebSocket(BASE.replace("http", "ws") + "/_quillo/wt?t=" + encodeURIComponent(ticket));
    await new Promise((ok) => ws.onopen = ok);
    ws.send(JSON.stringify(["j", "c"]));
    return ws;
  })));
  await sleep(3000);
  console.log(`round ${round}, ${N} pages in the channel:`, fp());
  for (const ws of pages) ws.close();
  await sleep(10000);
  console.log(`round ${round}, closed, 10 s on:`, fp());
}
process.exit(0);
```

`quillo run main.qwe --port 18995 --live-per-client 5000` (the default lets one address hold 256 pages, and every page here comes from 127.0.0.1), then `node pages.mjs http://127.0.0.1:18995 1000 <pid>`:

| | 89a91c2 | 80e5ea3 |
|---|---|---|
| before | 5.2 MB | 5.2 MB |
| 1,000 pages in the channel | 561 MB | 561 MB |
| closed, 10 s on | 389 MB | 386 MB |
| round 2: open, then closed | | 750 MB, then 519 MB |
| round 3: open, then closed | | 847 MB, then 600 MB |

About 556 KB for each page while it is open, and about 70% of it still held 10 s after every page closed. What stays after each round grows by less each time (+133 MB, then +81 MB), so it may be the allocator keeping freed memory rather than a leak, but three rounds were not enough to see it level off. In qwertime the same 1,000 pages cost 548 MB before they opened a room, and the room itself added 83 MB.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A server with a thousand pages on channels needs about half a gigabyte for them alone; qwertime's VPS has 2 GB for two app servers, PostgreSQL and limer |
| **Requests** | No change, until the memory runs out |
| **Security** | Anyone who can open pages can make the server hold half a megabyte each, up to `--live-per-client` pages per address (256 by default) |

**Suggestion:** look at what a connection allocates up front: a queue sized for its 4,096 messages, or the WebSocket's read and write buffers, made at their largest rather than as they are needed. A page that is quiet most of the time should cost a few kilobytes. And what a closed page held should go back to the system, or at least be used again for the next page.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `let room = channel.open("rooms", (m) => { … room.send(…) })` | the natural way to answer the server on a channel | `quillo check` passed; the test server never started (Q-950) | 1 full test run (about 5 min), and a rewrite to state and body statements |
| `state room = channel.open(…)` | the warning's advice (Q-952) | `unknown name room`, three times | 1 check |
| braces around `onclick={() => room.send(…)}` | guessed that the warning came from a handler giving back a value | no change | 1 check |
| bisecting the warning in copies of the program | the warning names no cause | found `js(...)` in the callback | about 25 checks |
| `view RoomDoor(…, key: str, …)` | `key` is the natural name for an invite key | refused, as Q-015's fix does; renamed to `invite_key` | 1 check |
| `fn invite_link(r)` in `lib/rooms.qwe` | did not look for the name; `lib/tools.qwe` already had one | "`invite_link` comes from lib/tools.qwe and lib/rooms.qwe: take the one you mean"; renamed to `room_invite_link` | 1 check |
| effects left off: a function that relays (`http`), and `crypto.token` (`random`) | my own slip | `quillo check` named each, with the function that needs it | 1 check |
| a helper `fn` for every map made in a lambda | unsure whether `(k) => { a: k }` is a map or a block | it is a map (`map([1, 2], (k) => { a: k })` gives `[{ a: 1 }, { a: 2 }]`); the helpers were not needed | none, only caution |
| `quillo check` alone for a component | it had passed before | not enough for components (Q-950): the server must start too | — |
| a load test of 1,000 pages from one machine | to see a room of a thousand | stopped at 256 pages: one address may hold 256 pages waiting (`--live-per-client`), and said so in the log. Production runs with `--trust-proxy 127.0.0.1`, so it counts each visitor's address | 1 run |

## Limitations

- **Channels are each server's** (USERGUIDE.md line 9147: "relay channels and streams across servers? No"). qwertime runs two servers behind limer, and a room's people can be on either, so the app relays for itself. It does this in about 150 of `lib/rooms.qwe`'s lines. Each server finds the others in limer's `/upstreams` (only the `discover qwertime` pool) and sends each change, line and list of who is here to them over `POST /rooms/relay` on 127.0.0.1. Each message is signed with a key made from `QUILLO_SECRET`, and refused if unsigned or a minute old. Copies of a room merge by the latest change to each person, so the order they arrive in does not matter. Each server asks the others for all their rooms every 10 s, so one restarted by a deploy catches up. limer cannot do this per message: it passes a page's WebSocket through unread after the `101`, so it can only pick a server per connection. Live data already crosses servers through limer's broadcast (`/_quillo/announce`). A relay for channels, opt-in (`channel.on(name, f, { relay: true })`) or always on under `--register`, would remove the need for an app's own. It would need more than limer's broadcast (64 KiB, fire-and-forget) for large messages.
- **Answering on a channel from its callback** is Q-950's workaround: state set in the callback, and body statements that send.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A page with no session joining a channel whose `join` asks for one | `["j","rooms",false]`, locally and in production through limer (427 ms from the ticket) |
| `join` sending to the page that is joining (Q-086's fix) | reaches it right after its yes; used for "hello", so the page re-opens its room after each reconnect |
| `channel.send` by peer id from an HTTP route (a form post asking into a room) | reaches the host's page at once |
| 46 messages sent in a burst on one WebSocket | handled in order: exactly the first 40 taken, under the app's own cap of 40 in 5 s, and the rest let go |
| `crypto.hmac(key, text)` | the same lowercase hex as Node's `createHmac("sha256", key).update(text)`, so servers and tests sign alike |
| `request.body` on a JSON POST | the raw text, for checking a signature |
| `http.post` to `127.0.0.1` between two servers (`public_only: false`) | works, in production too: each server answers the other's look every 10 s |
| An invite key compared with `crypto.equal` | a wrong key gets the listed room's door (asking), the right one comes straight in |
| A component with a channel, state set from `js(...)`, and `every` | resumed, not made again (`remade: 0`), no console errors, at desktop and phone sizes |
| `state zone = js("…")` | read in the browser once; each line showed the browser's local time |
| `date.from_ms(ms, zone)` | the same `HH:MM` in a server unit test and in Chrome |
| A body statement reading state (`if answer.n > 0 { … room.send … }`) | runs again on each change and sends; the page answers the host |
| `every()` at the program's top under `--no-jobs` | still runs (only `schedule` jobs stop) |
| A name with an emoji ("Sol ☀️") through the channel and the markup | kept and shown whole |
| `(k) => { a: k }` | a map, not a block |
