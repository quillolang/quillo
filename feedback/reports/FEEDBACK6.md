# Feedback from asking whether Quillo can make a video call

| | |
|---|---|
| App | None built: three questions asked in one session, each answered by probing (nine programs of 5 to 30 lines, kept as the repros below). Can Quillo make a video call? What does the command line do with no program named? How does [bank/](../../app/bank) chain its services, and what happens when a link of the chain is slow? |
| Quillo | 0.5.0 (72f7913) |
| Model | Claude Opus 5.5 |
| Tested with | headless Chrome (`feedback/tools/drive.mjs`), with a page script that hands out a canvas's `captureStream()` in place of a camera; `--split` with a chain of three services, one slow; the command line by hand, from a folder with a `main.qwe`; `feedback/run.mjs` for every repro |

What worked well: the parts of a video call Quillo already has held up. A `<video>` that a
`@mount` gave a `MediaStream` kept it, and kept playing, through state changes and a branch
appearing above it, so a caller's view of themselves is safe. `el.srcObject = stream` from
Quillo code passed the stream through untouched. A Promise from the page could be waited
for with `.then((s) => ...)`, a Quillo function as its callback, thanks to Q-050's fix.
`media.rtp` relayed one camera to six browsers (an earlier test of the user's), and it can
be called from a route to make a stream while the program runs. A service's `timeout`
under `--split` gave up at 1.01 s, as declared, and every call down a chain carries the
request's id.

What it cost: one silent failure that reaches far past video (Q-055: a keyed list written
with braces is not keyed, so a row added in front remakes every row, with what was typed
in them), one where a chain's caller is told a call failed while the work goes on beneath
it (Q-056), and a video call still needs hand-written WebRTC, because a page cannot send
its own camera (Q-060).

## Seen again

None. The issues met are all new; [INDEX.md](../INDEX.md) has nothing on keyed lists,
Promises, deadlines, the command line or sending media.

## New issues

### Q-055. `for x in xs { <li key={x}/> }` takes no notice of its key: a row added in front makes every row again

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser: a video call's participants, one added in front | 4 |

The guide says `for` in markup keeps a row for each item by its `key`, and that moved rows
keep their DOM and focus. That holds for `for p in items => <li key={p}/>`. Written with
braces, `for p in items { <li key={p}/> }`, the same list passes `quillo check`, renders the
same, and is kept **by place**:

| `items` goes from `["b", "c"]` to | `=>` | `{ ... }` |
|---|---|---|
| `["b", "c", "d"]` (one at the end) | b, c kept | b, c kept |
| `["a", "b", "c"]` (one in front) | b, c kept | every row made again |
| `["c"]` (the first one gone) | c kept | every row made again |
| reversed | both kept, moved | every row made again |

Made again means a new element: an `<input>` being typed in loses its focus and its text,
and a `<video>` loses the stream a `@mount` gave it (a call's participant goes black when
someone joins ahead of them). Components in the list (`<Tile key={p}/>`) are made again too.

Why: the compiler takes a `for`'s key only when its body *is* an element
([src/client.rs `each`](../../src/client.rs#L2943)). A block's element falls to `html()`,
for which `key` means "make it afresh when the key changes"
([src/client.rs:3054](../../src/client.rs#L3054)). The rows are then matched by place,
row 0's key goes from `b` to `a`, and so on down the list.

```quillo
component Rows() {
    state items = ["b", "c"]
    <div>
        <ul id="braces">{for p in items { <li key={p}>{p}</li> }}</ul>
        <ul id="arrow">{for p in items => <li key={p}>{p}</li>}</ul>
        <button id="front" @click={() => { items = ["a"] + items }}>add in front</button>
    </div>
}
```

Repro: [Q-055-key-in-a-for-block-is-ignored.qwe](../repros/Q-055-key-in-a-for-block-is-ignored.qwe)
(rows kept after "add in front": braces 0, arrow 2).

It took four runs to find: the first showed the video made again, the second the same for
`<li>` and `<input>`, the third ruled out the marker (a `data-` attribute Quillo might have
taken off; a JavaScript property on the element said the same), and the fourth, with `=>`,
kept every row.

**Suggestion:** take the key from a block whose value is one element, as from `=>`. Until
then, `quillo check` could say: "`key` inside a `for { }` block is not the list's: write
`for p in items => <li key={p}/>`".

### Q-056. A call given up on runs on below: the caller is told it failed while the services it called finish the work

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `--split`, a chain of three services, the last one slow | 1 |

`app` allows `b` 1 s; `b` allows `c` 3 s; `c` takes 2 s. Under `--split`, `app` gives up at
1.01 s, as declared, and tells its caller the call failed. `b` and `c` go on: a second later
`c` finishes and `b` prints "done, c answered", to nobody. No deadline goes down the chain
with the call (the request's id does), so `b` waits its own 3 s for an answer its caller
stopped waiting for long before.

```
gave up: service b: no answer from http://127.0.0.1:49847 (http://127.0.0.1:49847/_quillo/service/work:
         Network Error: Network Error: Error encountered in the status line: timed out reading response)
[c] POST /_quillo/service/work -> 200 (2002.4ms)
[b] b: done, c answered
[b] POST /_quillo/service/work -> 200 (2003.2ms)
```

In one process the same call takes 2 s and succeeds: the timeout is not applied. The guide
places timeouts under "when a service runs elsewhere", but it also says a program behaves
the same whichever way it runs, and a test run in one process never meets this path.

In [bank/](../../app/bank) the chain is gateway → payments (5 s, one retry) → risk (3 s) →
accounts (3 s). A retry is sent with the same call id and waits for the call still running,
which covers one slow link. If the ledger is slow for longer than both tries, the customer
is told the transfer could not be made while payments goes on to post it, and a second
press of "Send" is a new transfer. That is reasoned from this repro and the bank's code,
not run against the bank.

Repro: [Q-056-a-call-given-up-on-runs-on-below/](../repros/Q-056-a-call-given-up-on-runs-on-below)
(`main.qwe`, `b.qwe`, `c.qwe`; run with `--split`).

**Suggestion:** send what is left of the caller's time with each call, and make it the most
any call the service makes in turn may wait (its own `timeout` when that is shorter), so
the chain gives up together. The error could read as `http.get`'s does: "service b took
more than 1 s (its timeout)", without "Network Error" twice. The guide could say that in
one process `timeout`, `retries` and circuits do not apply.

### Q-057. `js(...)` of a function that gives a Promise gives the Promise, which `type_of` calls a map

| Kind | Detected by | Attempts |
|---|---|---|
| loud | browser console, attaching a camera | 2 |

`navigator.mediaDevices.getUserMedia` gives a Promise. A `server fn` call in component code
is awaited without a word, so I took `js(...)` to be awaited too:

```quillo
<video @mount={(el) => {
    let s = js("camLater")          // window.camLater = () => Promise.resolve(stream)
    el.srcObject = s
}}></video>
```

`s` is the Promise. `type_of(s)` says `map`, and setting it fails in the console only:
"quillo: Failed to set the 'srcObject' property on 'HTMLMediaElement': The provided value is
not of type '(MediaSourceHandle or MediaStream)'", which does not say a Promise was the
trouble. The guide's section on JavaScript libraries does not mention Promises.

What works: `js("camLater").then((s) => { el.srcObject = s })`.

Repro: [Q-057-js-gives-a-promise-unwaited.qwe](../repros/Q-057-js-gives-a-promise-unwaited.qwe)
(prints `kind=map`; `kind=str` once the Promise is waited for).

**Suggestion:** wait for a Promise that `js` and `js_new` give in component code, as for a
`server fn` (its rejection an error value). Short of that: `type_of` saying `promise`, and
a line in the guide with the `.then` form.

### Q-058. `quillo run --no-check app.qwe` takes the flag for the program

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the command line | 1 |

`run` takes its first argument as the program, whatever it is
([src/main.rs:110](../../src/main.rs#L110)):

| Typed | Said |
|---|---|
| `quillo run --no-check main.qwe` | ``quillo: unknown option `main.qwe` `` |
| `quillo run --watch` | `--watch: error: cannot read file: No such file or directory` |
| `quillo run main.qwe --no-check` | runs |
| `quillo check --strict-null main.qwe` | runs: `check` takes its flags anywhere |

"Cannot read file --watch" reads like a broken install, and `quillo run --watch` is what one
types first.

Repro: [Q-058-flag-before-the-program-taken-for-it.qwe](../repros/Q-058-flag-before-the-program-taken-for-it.qwe)
(`mode: cli`, new in [run.mjs](../run.mjs) for this).

**Suggestion:** read `run`'s, `app`'s and `build`'s options wherever they stand, as `check`
does: the program is the first argument that does not start with `--` (and is not an
option's value).

### Q-059. `quillo run` with no program, in a folder that has a `main.qwe`, prints all of the usage

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the command line | 1 |

Every app here is started as `quillo run app/<name>/main.qwe`, and every program is a
`main.qwe`. In such a folder:

| Typed | Said |
|---|---|
| `quillo` | the REPL, with no word of the `main.qwe` there |
| `quillo run`, `quillo check` | the 45 lines of usage, exit 2 |
| `quillo run .`, `quillo check .` | `.: error: cannot read file: Is a directory (os error 21)` |
| `quillo build .` | `quillo: cannot read .: Is a directory (os error 21)` |

`cargo run`, `npm start` and `go run .` work from the project's folder. Not guessing has
its point: [stays/](../../app/stays) and [bank/](../../app/bank) each hold a `seed.qwe`
beside the `main.qwe`, so "the only `.qwe` file" or "any `.qwe` file" would be wrong.

Repro: [Q-059-no-default-program/](../repros/Q-059-no-default-program) (`mode: cli`, `args: run`).

**Suggestion:** with no program, or a folder, `run`, `check`, `build` and `app` take that
folder's `main.qwe`, and nothing else; with none there, one line: "no program: name a .qwe
file, or add main.qwe here". The REPL could say "main.qwe is here: `quillo run` serves it".

### Q-060. A page cannot send its camera or microphone: media goes one way, from a device to pages

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the guide, `src/media.rs` and the runtime | – |

`media.rtp` takes H.264 and Opus over RTP from a camera or an encoder (ffmpeg), and
`<quillo-media>` plays it in any number of pages. The user's test showed that: one camera,
six browsers. Nothing goes the other way. The runtime never calls `getUserMedia`,
`VideoEncoder`, `AudioEncoder`, `MediaRecorder` or `RTCPeerConnection`, so a page cannot
publish a stream. A call where each person sends needs ffmpeg beside each browser (every
picture, one's own included, going through the server), or WebRTC written by hand, with
Quillo's channels carrying the offers, answers and ICE candidates.

The relay is the half that is there. A browser that sent its frames to the server once,
on the channel connection it has, would reach the other people in a call through the relay
that already reaches six (what video services call an SFU). A mesh, each browser sending to
every other, costs each of six people five uploads.

No repro: nothing to run.

**Suggestion:** `<quillo-media publish="name">`: the camera and microphone, encoded in the
browser with WebCodecs (H.264 baseline and Opus, the codecs the relay passes on already),
sent on the page's channel connection to the stream of that name. Who may publish could be
decided as a channel's `join` is (`media.on(name, { publish: (peer) => ... })`).

### Q-061. A stream can be made while the program runs, but not closed

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a route that calls `media.rtp` | 1 |

The guide says `media.rtp(name, ...)` "is written at the top of a program". In a route it
works too: `GET /open/1` made `room-1` on UDP port 15002, and asking again made no second
one. There is no `media.close`: `media` has `rtp` and `streams` only
([src/media.rs:41](../../src/media.rs#L41)). A stream per room, or per person in a call,
holds its UDP ports until the program stops, and each needs ports of its own.

No repro in `repros/`: what is missing cannot be shown failing.

**Suggestion:** say in the guide that `media.rtp` may run in a handler; add
`media.close(name)`, which stops listening and tells the stream's pages it ended. A stream
pages send (Q-060) would need no port.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| "No, Quillo has no video call" | The guide's media section | Right, but the user had seen one camera reach six browsers, and took it for a call. It took two more answers to say what a call needs that a broadcast does not | 3 answers |
| "Media streams are declared at the top of a program, with fixed ports" | The guide's words | `media.rtp` in a route works; what is missing is closing a stream (Q-061) | 1 probe |
| "A chain has no trace id" | I had read the bank, not the end of the guide's Services section | Every call to a service passes on the request's id; I said so in the report and to the user | read the guide |
| `type(s)` | Python, and Go's `reflect.TypeOf`; `type_of` is in the guide's list of builtins | ``unknown name `type` (did you mean `take`?)`` | 1 check |
| `for p in items { <li key={p}/> }` | JavaScript's and Rust's loops; the guide shows `=>` in markup but nothing says the brace form loses the key | `quillo check` passes; every row made again (Q-055) | 4 browser runs |
| `let s = js("camLater")`, used as a stream | `server fn` calls are awaited in component code | A Promise, called a map (Q-057) | 2 browser runs |
| `page.click("#b")` | Playwright's habit | `feedback/tools/chrome.mjs` clicks at coordinates: nothing was clicked, and the first run measured nothing | 1 run |
| `$Q $a` in a zsh loop | bash's word splitting | zsh passed `run --no-check main.qwe` as one argument, and "cannot read file" looked like Quillo's fault; caught before it went in the report | 1 run |
| `run.mjs`'s `serve` mode waits for "listening on" | It always had | Under `--split` a service says it first: the repro's request was refused. `run.mjs` now waits for the app's own port | 1 run |

## Limitations

- **A video call today** is hand-written WebRTC in a page script: `getUserMedia` (waited
  for with `.then`, Q-057), an `RTCPeerConnection` for each other person, and Quillo's
  channels for the signaling. The video goes from browser to browser and never through
  Quillo. A keyed list of participants must be written with `=>` (Q-055). Not tried end to
  end: two browsers connecting, and the offers (3 to 6 KB of SDP) going over a channel,
  where anything over about 1,100 bytes goes on a stream, reliably, by the guide.
- **Through Quillo**, a call is ffmpeg beside each browser, a `media.rtp` stream for each
  person, and a `<quillo-media>` for each stream: everyone sees everyone, a fraction of a
  second late, one's own picture too.
- Neither has TURN (a relay for people behind strict NATs) or recording.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A `<video>` given a `MediaStream` by `@mount`, then state changed and a `? :` branch appearing above it | the same element, the same stream, still playing |
| `el.srcObject = stream` from Quillo code | the `MediaStream` is passed through as it is: the video plays |
| `js("camLater").then((s) => { el.srcObject = s; status = "attached" })` | waited for; state set in the callback shows on the page |
| `for p in items => <video key={p}/>`, a participant added in front, then the first one gone | the others' elements and streams kept |
| `for p in items => <input key={p}/>`, someone typing, a row added in front | focus and text kept |
| `media.rtp("room-1", ...)` in a route, asked twice | one stream, no error |
| `timeout: "1s"` on a service, `--split` | gave up at 1.01 s |
| `quillo run main.qwe --no-check`, `quillo check --strict-null main.qwe` | both run |
| `quillo` alone | the REPL, as the usage says |
