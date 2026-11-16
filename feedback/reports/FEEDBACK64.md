# Feedback from learning Quillo on one counter: a `server fn`, a top-level `let`, and `live`

| | |
|---|---|
| App | `~/dev/quillo-manual/my-app/main.qwe`, outside this repo. It is the v0.8.1 starter cut down to one counter: an Up button in the browser, a Down button that calls a `server fn`, then a top-level `let`, and at the end a counter that every open tab shares live. 20 to 25 lines, one file. The user wrote it by hand, the way a newcomer would, and at each step asked why it did or did not work |
| Quillo | `quillo 0.8.1 (89a91c2, 2026-11-09)`. The starter's `.quillo/bin/quillo` and this repo's `target/release/quillo` (branch `speed-bun`, HEAD `89a91c2`) print the same version. The working tree has uncommitted changes to `src/httpd.rs` and `src/server.rs`; I did not check whether the release build includes them. Every repro below was run again on `target/release/quillo` |
| Model | Claude (Opus 5.5, `claude-opus-5-5`), explaining and probing. It wrote none of the app |
| Tested with | `quillo check`, `quillo repl`, and `quillo run` with and without `--watch`, using `curl`. 200 requests, 50 at a time (`xargs -P 50`). Headless Chrome through the DevTools protocol with two and three tabs, from a script of my own (at the end) rather than `feedback/tools/` |

No Quillo source was changed, and neither were `feedback/INDEX.md` or `feedback/repros/`. Each repro is in its own section. Numbers start at Q-931, after FEEDBACK63.md's Q-930. Under the rule of 2026-11-05, nothing here is built until main is in sync, every test (CI too) is green and the docs are aligned.

What worked well: a counter that every open tab shares live took one line, `state val = live(load_initial)`, over a top-level `let`, with no `watch` or `publish`. In two headless tabs, each click ran the loader once per tab and nothing ran between clicks. A fresh tab got the new value. The checker caught both wrong handlers (`onclick={up()}`, `onclick={val = down(val)}`), and each message named the fix. A dropped `push` or `val - 1` gets a warning that says what to write. I probed four facts about globals: every visitor shares them, 200 concurrent writes lose none, and they reset on restart and on any save under `--watch`. All four match USERGUIDE.md. Still, two of the user's three mistakes passed `quillo check` (Q-931, Q-932), and the third was refused without advice (Q-934).

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-505 | Not hit. Checked: a dropped `push` and a dropped `val - 1` warn, with the fix. Q-931 is what it leaves out | 0 |
| Q-359 | Not hit. Checked: `[a, b] = [b, a + b]` says what to write. Q-934 is a habit its list missed | 0 |
| Q-084, Q-090, Q-797 | Not hit. A `live` loader follows a global: one run per tab per write | 0 |

## New issues

### Q-931. A `server fn` whose answer is dropped changes nothing, and `quillo check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading the program, after the user saw the button do nothing | 1 |

The user's program:

```quillo
server fn down (val:int) {
        val-=1;
       return val
}

component Counter (cnt:int=0) {
    state val = cnt;
    ...
        <button onclick={()=>down(val)}>Down</button>
```

Clicking Down did nothing. The server got the call, worked out `val - 1` on its copy of `val` and answered. The handler dropped the answer, so the page's `state val` never changed. Inside the server fn, `val` is a parameter that shares the state's name, which reads as if it were the same variable. Nothing said so: `quillo check` was ok, the log showed `POST /_quillo/rpc/down -> 200`, and the console was empty.

Q-505's warning covers builtins and operators, not the program's own functions:

```quillo
server fn down(val: int) -> int = val - 1
fn minus_one(v: int) -> int = v - 1

component Counter() {
    state val = 0
    <div>{val}
        <button onclick={() => down(val)}>server fn, dropped</button>
        <button onclick={() => { minus_one(val) }}>fn, dropped</button>
        <button onclick={() => { val - 1 }}>operator, dropped</button>
    </div>
}

route GET / = <Counter/>
```

```
drop.qwe:9: warning: `val - ...` is worked out and dropped: to change `val`, `val -= ...`
drop.qwe: ok (1 route, 2 functions, 1 component), 1 warning
```

Cost: one question from the user. The AI found the cause by reading the program, and checked `val = down(val)` with `quillo check`. The user then saw it work.

| Impact | |
|---|---|
| **Functionality** | The button does nothing |
| **User** | A developer sees a button that does nothing, and no message anywhere: not in `check`, the log or the console |
| **Requests** | Each click is a `POST /_quillo/rpc/down` whose answer is thrown away |
| **Security** | No change |

**Suggestion:** extend Q-505's warning to calls of the program's own functions, `server fn`s included, that cannot change anything. Such a function has no `with` effect, assigns no global, does not `publish`, and calls nothing that does. For example: ``warning: `down` gives a value and changes nothing, and it is dropped here: write `val = down(val)` ``. For a `server fn`, the message can add what a newcomer misses: ``its arguments are copies: the server cannot change the page's `val` ``. A call to a function with an effect stays silent. In the live counter at the end of this session, `() => { down() }` drops the answer on purpose, since `down` assigns a global that `live` follows.

### Q-932. `quillo check` passes a list's `set` given a list or a text as its index; it fails when it runs

| Kind | Detected by | Attempts |
|---|---|---|
| loud (late) | run-time error (a 500 from a `server fn`), then the REPL | 1 |

After Q-934 refused `initial[0] = val`, the user wrote `initial = set(initial,[0],val);` in `down`. `quillo check` said ok, and every click was a 500 (see Q-933 for what the page showed).

```quillo
let xs = [1, 0]
fn f() = set(xs, [0], 5)
route GET / = f()
```

The checker does check builtins' argument types. It just does not check `set`'s index:

| Call | `quillo check` | When it runs |
|---|---|---|
| `set([1, 0], [0], 5)` | ok | ``error: `set` expects an int as argument 2, got list`` |
| `set([1, 0], "a", 5)` | ok | ``error: `set` expects an int as argument 2, got str`` |
| `set({ a: 1 }, 5, 2)` | ok | `{ a: 1, "5": 2 }` (an int key becomes text, Q-019) |
| `len(5)` | ``error: argument 1 of `len` must be str \| list \| map \| html \| null, got int`` | error |
| `push(5, 1)` | ``error: argument 1 of `push` must be list \| null, got int`` | error |
| `join([1, 2], 5)` | ``error: argument 2 of `join` must be str, got int`` | error |

`upper(3)`, `split(5, ",")`, `range("a", 3)` and `pow("2", 3)` are caught too. What `set`'s index may be depends on its first argument, and the checker takes any index.

| Impact | |
|---|---|
| **Functionality** | The call fails every time: in a `server fn`, a 500 |
| **User** | A visitor sees `internal error`. Under `--watch`, the developer sees ``internal error in `down` `` on the page and has to find the terminal (Q-933) |
| **Requests** | `POST /_quillo/rpc/down -> 500` on each click |
| **Security** | No change |

**Suggestion:** when the type of `set`'s first argument is known, check its index against it. A list takes an int: ``argument 2 of `set` on a list must be int, got list``. A map takes any key (an int becomes text, Q-019). I did not look for other builtins whose argument types depend on another argument.

### Q-933. Under `--watch`, a `server fn`'s run-time error reaches the page as "internal error in `down`", while a route's shows its cause

| Kind | Detected by | Attempts |
|---|---|---|
| loud | browser (headless Chrome), then the server's stderr | 1 |

The user ran `./run.sh --watch` and pasted what the page showed:
``Hello1 err("server fn `down` failed (500): internal error in `down`")UpDown``. The cause was in the terminal, which they did not think to read.

```quillo
let initial = [1, 0]

server fn down(val: int) {
    val -= 1
    initial = set(initial, [0], val)
    return val
}

component Counter(cnt: int = 0) {
    state val = cnt
    <div>Hello1 <span id="v">{val}</span>
        <button id="down" onclick={() => val = down(val)}>Down</button>
    </div>
}

route GET / = <html><body><Counter cnt={initial[0]}/></body></html>
route GET /boom = set([1, 0], [0], 5)
```

I clicked Down in Chrome and requested `/boom` with `curl`, in both modes:

| | Page, without `--watch` | Page, under `--watch` |
|---|---|---|
| Route `/boom` | `internal error` | ``internal error: …/err.qwe:17: `set` expects an int as argument 2, got list`` |
| `server fn` `down` | `Hello1 err("internal error")Down` | ``Hello1 err("server fn `down` failed (500): internal error in `down`")Down`` |

The server's stderr was the same in both modes:

```
quillo: error in server fn `down`: …/err.qwe:5: `set` expects an int as argument 2, got list
  in down
POST /_quillo/rpc/down -> 500 (0.2ms)
```

The browser console had nothing in either mode.

The guide says both things. USERGUIDE.md:798 says "A runtime error says where it happened and how it got there, in the terminal, the log and, under `--watch` [...], on the page". USERGUIDE.md:2610 says "500 `internal error in x`, whose real message is on the server's stderr". The second is what happens. The first holds only for routes.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | Under `--watch`, neither the page nor the console gives the cause, so the developer has to think of the terminal. This user did not, and asked instead. Because the program stored the answer in `val`, the error value became the counter's text, so outside `--watch` a visitor reads `err("internal error")` on the page. That part is the program's doing (it has no `is_err`), and the guide's `Tasks` example shows the check |
| **Requests** | No change. Under `--watch`, the 500's body could carry the cause, as a route's does |
| **Security** | No change, if the cause goes out under `--watch` only, as it does for routes |

**Suggestion:** under `--watch`, put the cause and its place in a `server fn`'s 500, so the error value reads ``server fn `down` failed (500): main.qwe:9: `set` expects an int as argument 2, got list``. Also print it in the console with its `.qwe` line, as Q-173 did for errors in the browser. Outside `--watch`, keep today's behaviour. Until then, USERGUIDE.md:798 could say that this applies to a route's page.

### Q-934. `xs[0] = v`, `m[k] = v` and `m.k = v` are refused without saying what to write

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

The user wrote `initial[0]=var;`, where `var` was a typo for `val`. That typo was hidden, since the check stops at this one error. The message says what is not allowed, but not what to write:

```quillo
let xs = [1, 0]
let m = { k: 1 }
fn f() {
    xs[0] = 5
    1
}
route GET / = f()
```

| Written | Message today | What it could say |
|---|---|---|
| `xs[0] = 5` | `` only variables, and the fields of the page's objects (`el.value = x`), can be assigned to `` | ``a list does not change in place: `xs = set(xs, 0, 5)` `` |
| `xs[0] += 1` | the same | `` `xs = set(xs, 0, xs[0] + 1)` `` |
| `m["k"] = 5` | the same | `` `m = { ...m, k: 5 }` or `m = set(m, "k", 5)` `` |
| `m.k = 5` | ``cannot set `k`: only the page's objects (an element, an event) have fields that are set, in a component`` | `` `m = { ...m, k: 5 }` `` |

The other assignment habits now get the fix. Since Q-359, `[a, b] = [b, a + b]` gives ``several names are not assigned at once: `let [c, d] = [b, a + b]`, then assign each``. With no hint, the user went looking, wrote `set(initial,[0],val)`, and hit Q-932. Once `initial = set(initial,0,val)` worked, they called it awkward.

| Impact | |
|---|---|
| **Functionality** | No change: refused at `check` |
| **User** | A newcomer has to find `set` among the builtins. This one guessed the form of its index wrong |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** the messages in the table. There is also a choice for the language: take `xs[i] = v` (and `xs[i] += v`) on a variable to mean `xs = set(xs, i, v)`, the way Swift treats an item of a value type. Nothing would change in place. A list read before the assignment keeps its old items (checked below: `{"mine":[1,0],"now":[5,0,9]}`), and Q-352's update in place still applies. `m.k = v` is harder, since in a component it already sets a page object's field (Q-009), so the same text would mean two things depending on what `m` holds. The message for `xs[0] = 5` names only `el.value = x`, so `xs[i] = v` seems free of that clash. I did not check that against the page's objects.

### Q-935. llms.txt says nothing of what a global is

| Kind | Detected by | Attempts |
|---|---|---|
| docs | probes, then reading USERGUIDE.md | 4 |

The starter's AGENTS.md says to read https://quillolang.org/v0.8.1/llms.txt "before writing any", and its source here is `book/llms.md`. The user's questions were "no variable can be defined outside the function right?", "so it can be a runtime storage", "the developer needs to watch out ... because it is shared", and "how to put live update for the let variable?". Each took a probe, since llms.txt answers none of them. USERGUIDE.md answers all of them:

| Fact | USERGUIDE.md | llms.txt (and `book/llms.md` on HEAD) |
|---|---|---|
| A top-level `let` is one value for every request and visitor | implied throughout | nothing |
| It starts afresh at restart | 8972: "No: globals start afresh; keep it in the database" | nothing |
| It starts afresh on any save under `--watch` (probed: a comment-only edit, 3 to 0) | not found | nothing |
| One handler runs at a time between waits, so `hits += 1` loses nothing | 7701: "safe without locks, since only one handler runs at a time" | nothing |
| Each `--instances` copy has its own | 7707: "each instance has its own variables" | nothing |
| `live` follows a global | 2372 to 2373, 2379 (`var:todos`), 3285 | only "again when what it read changes", with a table in the example |

```sh
grep -n -i -E 'global|top[- ]level|instances' book/llms.md
# 209:- `schedule("0 3 * * *", job)` and `every(50, tick)` go at the top level of the program;
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | An assistant that reads only llms.txt either guesses or probes. I probed four times, and before probing I told the user something about concurrency the guide settles ("I can't promise") |
| **Requests** | No change |
| **Security** | A model that does not know a global is shared by every visitor may keep one visitor's data in one, where the next visitor sees it. Q-166 is the same mistake across tenants |

**Suggestion:** a few lines in `book/llms.md`, for example:

```
- A top-level `let` is one value for the whole server: every request and visitor sees
  it, a `live` loader that reads it runs again when it is assigned, and it starts afresh
  at a restart and on each save under `--watch`. Each `--instances` copy has its own.
  One handler runs at a time between waits, so `hits += 1` loses nothing, but a value
  read before a `db.*` or `http.*` call may have changed after it. Keep what must last,
  or belongs to one visitor, in the database or the session.
```

## Where the AI struggled

The user's own steps are in the sections above. These rows are the session in order, marked by whose mistake each was.

| What was written first | Why | What happened | Cost |
|---|---|---|---|
| (user) `onclick={()=>down(val)}`, with `val-=1` in the `server fn` | Expected the server to change the page's `val` | Nothing, and no message (Q-931) | 1 question |
| (AI, through a fetch subagent) "the guide's Counter takes `start`, so `cnt={0}` passes a prop it does not declare" | It compared the user's component with the guide's | Wrong: the program declares its own `cnt`, and `quillo check` was ok. Dropped before answering | 0 |
| (user) "no variable can be defined outside the function right?" | No top-level variables in what they had seen | A top-level `let` works, and functions can assign it | 1 probe |
| (AI) "I can't promise" about writes from requests at once | Read only llms.txt, as AGENTS.md says | USERGUIDE.md:7701 promises it (Q-935) | 1 probe of 200 requests |
| (user) `initial[0]=var;` | Habit from other languages | Refused, without a hint (Q-934). The `var` typo was hidden behind it | 1 check |
| (user) `initial = set(initial,[0],val);` | Guessed the form of `set`'s index | `check` ok, then a 500 with no cause on the page (Q-932, Q-933) | 1 question, 1 REPL line |
| (AI) "the real error should appear in the terminal" | The guide's `server fn` text | Said before checking. It does | 1 later probe |
| (AI) "the checker doesn't verify every builtin's argument types" | One miss, `set` | Too broad: it checks `len`, `upper`, `split`, `push`, `range`, `join` and `pow`. Narrowed to Q-932 | 1 probe |
| (AI) a `live` test with `watch` and `publish` beside one without | Expected the topic to be needed for a global | Both behave the same. The guide says why (2372 to 2373) | 1 browser run |
| (AI) `fn dec(v)` in a probe | A short name | ``warning: `dec` hides the builtin `dec` ``, so it was renamed `minus_one` | 1 rerun |
| (AI) "every save under `--watch` resets a global" | Guessed from "reloads the program" | Told the user before checking. It is true, for a comment-only save too | 1 later probe |

## Limitations

- There is no shorthand for changing one item of a list, or one field of a map, held in a variable. The workarounds are `xs = xs.set(0, v)` (or `xs | set(0, v)`) and `m = { ...m, k: v }`. See Q-934.
- Globals live in memory. They are not kept across a restart or a save under `--watch`, and each `--instances` copy has its own. This is by design (USERGUIDE.md:8972), and the workaround is the database.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `onclick={up()}` | ``error: `@click` needs a function, got null: a handler is a function, `@click={() => ...}` (or `onclick`); text is never run as code`` |
| `onclick={val = down(val)}` | ``error: an assignment is not a value: a handler is a function, `@click={() => { val = ... }}` `` |
| `onclick={up}` (a named function with no parameters), `onclick={() => val = down(val)}` (no braces), `@click={...}` | ok |
| `val -= 1` on a `server fn`'s parameter | ok: a parameter is a variable |
| A top-level `let` read by a route, a `server fn` and a component, and passed as a prop (`<Counter cnt={initial}/>`) | ok: the server renders `<div>Hello1 1...` |
| A top-level counter whose function reads it, computes `sum(for i in 0..20000 => i)`, then writes it back: 200 requests, 50 at a time | 200: none lost |
| The same counter after a restart | 0, as the guide says |
| `let mine = xs`, then `xs = set(xs, 0, 5) \| push(9)` | `{"mine":[1,0],"now":[5,0,9]}`: a list already read never changes |
| `state val = live(load_initial)` over a global, with no `watch` or `publish`, in two tabs, clicking Down twice in the first | Both tabs read 0, -1, -2. After each `down`, one `load_initial` per tab and nothing between clicks. A third tab opened afterwards reads -2 |
| The same with `watch("initial")` in the loader and `publish("initial")` after the write | The same, with no extra loads |
| A `server fn`'s run-time error, on the server | stderr names the file, the line and `in down`, in both modes |
| A dropped `push`, a dropped `val - 1` | Warnings that say what to write |

## The scripts

The two-tab test. Node 22 or newer. Chrome started with `--headless=new --remote-debugging-port=9333`:

```js
// usage: node twotabs.mjs <chrome-debug-port> <page-url>
// Opens two tabs, clicks #down in the first twice, prints #v in both, then opens a third.
const [port, url] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function tab(u) {
  const r = await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(u)}`, { method: "PUT" });
  const ws = new WebSocket((await r.json()).webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  let id = 0;
  const pend = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); }
  };
  const send = (method, params = {}) => new Promise((res) => {
    const i = ++id; pend.set(i, res); ws.send(JSON.stringify({ id: i, method, params }));
  });
  const ev = async (expression) =>
    (await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;
  return { ev, close: () => ws.close() };
}

const read = (t) => t.ev(`document.getElementById("v")?.textContent`);
const show = async (label, t1, t2) => console.log(`${label.padEnd(26)} tab1=${await read(t1)}  tab2=${await read(t2)}`);

const t1 = await tab(url);
const t2 = await tab(url);
await sleep(1500);
await show("loaded", t1, t2);
for (const n of [1, 2]) {
  await t1.ev(`document.getElementById("down").click()`);
  await sleep(2000);
  await show(`after Down #${n} in tab1`, t1, t2);
}
const t3 = await tab(url);
await sleep(1500);
console.log(`${"new tab3 (fresh load)".padEnd(26)} tab3=${await read(t3)}`);
for (const t of [t1, t2, t3]) t.close();
```

The program it drove:

```quillo
let initial = 0

server fn load_initial() -> int = initial

server fn up() -> int {
    initial += 1
    initial
}

server fn down() -> int {
    initial -= 1
    initial
}

component Counter() {
    state val = live(load_initial)
    <div>Hello1 <span id="v">{val}</span>
        <button id="up" onclick={() => { up() }}>Up</button>
        <button id="down" onclick={() => { down() }}>Down</button>
    </div>
}
route GET / = <Counter/>
```

Q-933 used the same connection on one tab, with `Runtime.enable` and listeners for `Runtime.consoleAPICalled` and `Runtime.exceptionThrown`. The concurrency test:

```sh
seq 200 | xargs -P 50 -I{} curl -s -o /dev/null http://127.0.0.1:8096/hit
curl -s http://127.0.0.1:8096/peek    # 200
```
