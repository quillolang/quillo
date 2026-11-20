# Feedback from making Quillo's courses: findings passed on by a second session, reproduced

| | |
|---|---|
| App | No app of this report's own. Five findings from quillo-videos-36 while it made the Advanced course (Help Desk, `~/dev/quillo-videos/advanced/`) and the Database course (`~/dev/quillo-videos/database/`), each reproduced for this report in a few lines of its own (the repros below); two of quillo-videos-37's own (Q-1265, Q-1266), from video 175, Trail Club's chat (`~/dev/quillo-videos/videos/175-bun-chat`); and three more from quillo-videos-36's React Quirks #1 (Q-1267 to Q-1269; the block's last; two more went in FEEDBACK81). |
| Quillo | `quillo 0.8.6 (238dac2, 2026-11-16)`, the version the courses are pinned to, and `quillo 0.8.8 (47869ad, 2026-11-20)`, this repository's `target/release/quillo` at the time: all four bugs open on both; Q-1264 is the guide's. Q-1265 to Q-1269 on 0.8.6 and `quillo 0.8.8 (6e18cda, 2026-11-20)`: open on both. |
| Model | Claude Opus 5.5 (`claude-opus-5-5`), as quillo-videos-37, with the findings from quillo-videos-36 (also Claude Opus 5.5) |
| Found by | quillo-videos-36, Q-1260 to Q-1264 and Q-1267 to Q-1269; reproduced and written up by quillo-videos-37, who found Q-1265 and Q-1266. FEEDBACK79 (Q-1250 to Q-1259) was full. |
| Tested with | `quillo check`, curl, `--metrics`, `sqlite3` on a copy of a database file, Google Chrome (headless, through `tests/browser/run.js`), Playwright's Chromium, `lsof`, `feedback/run.mjs` for the repros. |

What worked well: the courses ran on these programs for days of lessons, with real databases, services and feeds.
`vacuum into` made a good, whole backup every time; `db.migrate` applied every lesson's migration; a route's
`etag` gave real 304s to browsers and curl; the effect system's message at run time named the function, the effect
and the route exactly. What follows is where each fell short, found because a lesson's shots or a restore didn't
come out as the lesson said they would.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| – | none | – |

## New issues

### Q-1260. On a clean stop, each SQLite database keeps its WAL, and a restore copied over the file is undone without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a restore in the Database course's lesson 20 that came back empty; then `ls` and `sqlite3` on the files | 1 |

The guide says to back SQLite up "with its own command, not by copying the file while it runs" (`vacuum into` or
`.backup`), which reads as: a stopped program's file is whole, and safe to copy or replace. It isn't. On a clean stop
(SIGINT, the process gone), `quillo run` leaves each database's `-wal` and `-shm`, and the main file without what is
still in the WAL: here `library.db` is 4096 bytes and `library.db-wal` 45 KB, and a copy of `library.db` alone says
`no such table: loans`. Opening and closing the file elsewhere checkpoints it, as SQLite does when its last
connection closes, so the server seems to exit without closing its connections.

Worse, a restore: a good backup taken with `vacuum into` (3 loans), a mistake (`delete from loans`), a clean stop, and
the backup copied over `library.db`. The next start replays the stale WAL over the restored file, and the count is 0.
The restore is undone, and nothing says so. Removing `library.db-wal` and `-shm` before copying the backup in works.

```quillo
db.open("library.db")
db.migrate("migrations")
route POST /loan with db { db.exec("insert into loans (book) values ('a book')")?; "ok" }
route POST /backup with db { db.exec("vacuum into 'backups/good.db'")?; "backed up" }
route POST /mistake with db { db.exec("delete from loans")?; "deleted" }
route GET /count with db = db.one("select count(*) as n from loans").n
```

Three loans, a backup, the mistake, SIGINT; `library.db`, `library.db-wal` and `library.db-shm` are all there; the
backup copied over `library.db`; started again: `count after the restore: 0`, on 0.8.6 and 0.8.8
(`repros/Q-1260-clean-stop-leaves-wal-restore-undone/`, with `steps.sh`).

| Impact | |
|---|---|
| **Functionality** | A stopped program's database file is not whole; a restore by copying, the obvious way, is lost at the next start. |
| **User** | Whoever restores after a mistake believes the data is back, and it's the mistake that's back. |
| **Requests** | No change. |
| **Security** | No change; but it is data loss, at the moment someone is trying to undo one. |

**Suggestion:** close each SQLite database on a clean stop (a checkpoint, then close), so the file is whole when
the program has ended. And say in the guide that a restore removes the `-wal` and `-shm` first, or give `db` a
restore that does it.

### Q-1261. `quillo check` misses an effect a route needs through a component's state initializer

| Kind | Detected by | Attempts |
|---|---|---|
| loud | a 500 in the Advanced course's lesson 19, only once a ticket had a reply (so only in the shots) | 1 |

`quillo check` asks a route for the effects the functions it calls need: a route `with db` that calls a `fn ... with
env` itself fails check with ``main.qwe:3: error: `site_url` needs the env effect, but `route GET /sitemap.xml` is
declared `with db` ``. Not when the call
goes through a component's `state x = load()`, which the server runs while it renders the route's page: there,
check says ok, and the page is a 500 (`internal error`), the log saying ``main.qwe:3: `stamp` needs the time
effect, but `route GET /` is declared `with db` ``.

```quillo
fn stamp() -> str with time = str(now())
server fn load_it() -> str = stamp()
component Shows() {
    state v = load_it()
    <p>{v}</p>
}
route GET / with db = <html><body><Shows/></body></html>
```

`quillo check`: `main.qwe: ok (1 route, 2 functions, 1 component)`; `GET /` is a 500, on 0.8.6 and 0.8.8
(`repros/Q-1261-effect-through-component-state/`). In the course, the ticket page's component loaded `load_ticket`,
a server fn that calls `ago`, which is `with time`; it ran only for a ticket with a reply.

| Impact | |
|---|---|
| **Functionality** | A page whose component loads data through a function with another effect fails at run time, sometimes only on some data. |
| **User** | A visitor gets "internal error"; the developer, a clean check. |
| **Requests** | A 500 for the page. |
| **Security** | No change. |

**Suggestion:** follow the server fns a component's state initializers call (the server runs them while it renders
the route's page) and ask the route for their effects, as check does for the route's own calls.

### Q-1262. A route's etag makes the answer a 304, but the log says 200 and the metrics count a 2xx

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl beside the log, in the Advanced course; the metrics checked for this report | 1 |

The guide: `respond(200, v, { etag: ... })` is "a 304 to a GET or HEAD whose If-None-Match holds it, as files are".
It is: curl gets 304, and a browser's reload keeps its cached body. But the request's log line says 200, and
`--metrics` counts it as a `2xx` (its `3xx` stays 0).

```quillo
route GET /help/:slug = respond(200, <html><body><h1>{slug}</h1></body></html>, { etag: `"${slug}"` })
```

The page, then again with `If-None-Match: "hours"`: curl prints 200, then 304; the log has `GET /help/hours -> 200`
twice, and `quillo_requests_total{status="2xx"} 2`, `{status="3xx"} 0`, on 0.8.6 and 0.8.8
(`repros/Q-1262-etag-304-logged-as-200/`).

| Impact | |
|---|---|
| **Functionality** | No change on the wire. |
| **User** | Whoever reads the log or a dashboard can't see that caching works, or (the course's lesson) that a stale etag is serving old pages. |
| **Requests** | The log and the metrics say 200 for answers sent as 304. |
| **Security** | No change. |

**Suggestion:** log and count the status that was sent.

### Q-1263. `vacuum into` counts its path from the working folder, while `db.open` counts from the program's

| Kind | Detected by | Attempts |
|---|---|---|
| loud | a 500 in the Database course, started from another folder | 1 |

`db.open("lib.db")` counts its path from the main file's folder, as the guide says paths do. A backup with
`db.exec("vacuum into 'backups/b.db'")` (the guide's own way to back up) is SQLite's, and counts from the process's
working folder. Started from another folder (as `run.sh`, which doesn't `cd`, starts it), the backup is a 500:
`unable to open database: backups/b.db`.

```quillo
db.open("lib.db")
route POST /backup with db {
    db.exec("vacuum into 'backups/b.db'")?
    "backed up"
}
```

`quillo run app/main.qwe` from the folder above, then `POST /backup`: `{"error":"internal error"}` and
`unable to open database: backups/b.db` in the log, on 0.8.6 and 0.8.8
(`repros/Q-1263-vacuum-into-path-from-working-folder/`).

| Impact | |
|---|---|
| **Functionality** | A backup that works from the program's folder fails from any other. |
| **User** | A scheduled backup started by a service manager (another working folder) fails; someone has to read the log to know. |
| **Requests** | A 500 for the backup. |
| **Security** | No change. |

**Suggestion:** say it in the guide beside the `vacuum into` advice ("a path in SQL counts from the working folder"),
or give `db` a `backup(path)` that counts from the program's folder, as `db.open` does.

### Q-1264. The guide doesn't say that `verify_password(pw, null)` does the full work, nor that a null check before it is what tells members from strangers

| Kind | Detected by | Attempts |
|---|---|---|
| docs | timing sign-ins (quillo-videos-36's Security course, lesson 3; measured again for this report) | 2 |

How long a sign-in takes can tell an attacker whether an email has an account. Quillo already guards the guide's
plain form: `crypto.verify_password(password, user.password_hash)` takes its full time (about 10 ms here) when the
hash is null, as for an unknown email, so `if !crypto.verify_password(password, user.password_hash) { reply 401 … }`
("Cookies and sessions") answers in the same time for members and strangers. Nothing in the guide says so, and the
natural way to write it, with a null check first, does leak: `if user == null || !crypto.verify_password(…)` never
reaches `verify_password` for an unknown email, which answers at once. That is what the Security course's first
version did; and its author, reading the guide's other examples (`let hash = user.password_hash ??
crypto.hash_password("not anyone's")   // same work, user or not`), took a dummy hash to be needed, which it isn't
(it's harmless as written: one hash for a stranger, one verify for a member).

```quillo
// the guide's plain form: no split
let user = db.one("select * from users where email = ?", email)
if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }

// a null check first: a stranger never reaches verify_password, and answers at once
if user == null || !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }

// verify every time, then decide: no split
let ok = crypto.verify_password(password, user?.password_hash)
if user == null || !ok { reply 401 "Wrong email or password" }
```

Measured with curl, the fastest of three for a member's email and an unknown one, both with a wrong password
(Argon2id, the defaults; the machine was busy, so the milliseconds vary between 12 and 22):

| Form | 0.8.6 (238dac2): member, unknown | 0.8.8 (47869ad): member, unknown |
|---|---|---|
| a route that does nothing (baseline) | 0, 0 ms | 0, 0 ms |
| the guide's plain form | 20, 18 ms | 19, 18 ms |
| the guide's "same work" form (`?? crypto.hash_password(…)`) | 18, 22 ms | 12, 17 ms |
| a hash made once at the top as the fallback | 20, 17 ms | 17, 17 ms |
| **a null check first** | **19, 0 ms** | **18, 0 ms** |
| verify every time, then the null check | 13, 17 ms | 18, 17 ms |

`verify_password` alone, timed in a script: 10 ms with a real hash and 10 ms with null, on both. quillo-videos-36
measured the same on f35bf2d (a real hash, null and plain text alike).

| Impact | |
|---|---|
| **Functionality** | No change: the guide's own forms are safe. |
| **User** | A developer who adds the obvious null check first leaks who has an account; one who reads the "same work" comment adds a hash per stranger that isn't needed. |
| **Requests** | With a null check first, a stranger's sign-in answers in 0 ms against a member's ~18. |
| **Security** | Account enumeration by timing, in code that looks careful. |

**Suggestion:** beside the sign-in example, say that `verify_password` takes the same time for a null or broken hash,
so the plain form is safe; that a `user == null ||` before it is what leaks; and show "verify every time, then decide"
as the way to keep a null check. The `?? crypto.hash_password("not anyone's")` lines can then go.

### Q-1265. On a clean stop, the server leaves its WebTransport sessions open: after a restart, a page that only listens hears nothing for 10 to 20 s, and a page's first line is lost

| Kind | Detected by | Attempts |
|---|---|---|
| silent | restarting the server under two open tabs, for video 175 (Trail Club's chat, `~/dev/quillo-videos/videos/175-bun-chat`); found by quillo-videos-37 | 3 |

The guide says a page's connection "is opened again when it drops (after half a second, then longer, up to 15 s)".
Over the WebSocket that holds: a restart closes the TCP connection, and in the video's runs both tabs were back
1.2 to 1.7 s after the server started again (5 rounds of 5). Over WebTransport, which a page takes wherever it can,
the server stops (SIGTERM) without closing its sessions, so a page only learns the session is gone from the
keep-alive ("dropped after 20 s of silence"). Until then it hears nothing, and what is broadcast meanwhile is lost
to it.

```quillo
route POST /say { form: { text: str } } {
    channel.broadcast("chat", text)
    respond(204, null)
}

component Listen() {
    state lines = []
    let chat = channel.open("chat", (line) => { lines = lines + [line] })
    <ul id="lines">{for l in lines => <li>{l}</li>}</ul>
}

route GET / = <html><body><Listen/></body></html>
```

`repros/Q-1265-clean-stop-leaves-webtransport-pages-deaf`: the page opened at `127.0.0.1` in Chrome (WebTransport),
the program stopped 4 s in and started again, then `curl -d text=N /say` once a second from 1. The page's first
line was **post 11 or 12** on 0.8.6 (238dac2) and on 0.8.8 (6e18cda): ten or more lines lost, and ten seconds of
silence. The same repro with the page at `localhost` (the WebSocket, Q-1266) heard **post 1** on both.

With two tabs in Playwright's Chromium (Ana listening, Ben saying "ping N" every 2 s after the restart), on both
versions: Ana first heard Ben 23 s after the restart (ping 10). Ben's page came back within a few seconds, since
sending on the dead session showed it was gone, but his **"ping 1" appeared nowhere**: sent into the old session,
not kept for the new one, as the guide says lines sent "while the connection is down" are. His pings 2 to 9 reached
his own tab and not Ana's.

| Impact | |
|---|---|
| **Functionality** | Channels come back by themselves, but late: after every restart or deploy, 10 to 23 s with nothing heard. |
| **User** | Pages that only listen (a live board, a chat someone reads) miss everything said meanwhile; the first line someone types after a deploy disappears, with no error. |
| **Requests** | No change. |
| **Security** | None. |

**Suggestion:** on a clean stop, close every WebTransport session (with a code that says the server is going), so
pages reconnect at once, as they do from the WebSocket's close. And keep a page's line that was sent on a session
found dead, to send again on the new one, as lines sent while down already are. Until then, the Channels section
could say that over WebTransport a restart is noticed by the keep-alive, up to 20 s later.

### Q-1266. WebTransport listens on 127.0.0.1 alone and the ticket names no host: a page at `http://localhost` dials `[::1]` and is refused, so development never uses WebTransport

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a console error in every page of video 175's chat at `localhost`, and `lsof` (quillo-videos-37) | 2 |

Started without `--host`, the program listens on `127.0.0.1` for TCP and, once a page asks for a ticket, on
`127.0.0.1` for UDP. The ticket (`POST /_quillo/wt-ticket`) gives only `{"ticket", "port", "hash"}`, so a page at
`http://localhost:8175` dials `https://localhost:8175/_quillo/wt`. Chrome resolves `localhost` to `[::1]` for QUIC,
where nothing listens: `Failed to establish a connection to https://localhost:8175/_quillo/wt?…:
net::ERR_CONNECTION_REFUSED`, and the page takes the WebSocket for the rest of its life. TCP to `localhost` falls back
to `127.0.0.1` by itself; QUIC does not. The same page at `http://127.0.0.1:8175` gets WebTransport, with no error and
no WebSocket.

`repros/Q-1266-webtransport-at-localhost-refused` asks for a ticket and prints what the program listens on, on 0.8.6
and 0.8.8 alike:

```
{"ticket":"…","port":18974,"hash":"…"}
TCP 127.0.0.1:18974
UDP 127.0.0.1:18974
```

| Impact | |
|---|---|
| **Functionality** | Channels work (over the WebSocket), but never over WebTransport at `localhost`, the address the guide's examples use. |
| **User** | A console error on every page load in development; and the WebTransport path, with Q-1265, is seen first in production. |
| **Requests** | No change. |
| **Security** | None. |

**Suggestion:** when the program listens on loopback, listen for UDP on `[::1]` too, or put a `host` in the ticket
(`127.0.0.1` for a page at `localhost`) for the page to dial. Or, at least, add "the page is at `localhost`" to the
Channels section's list of reasons a page uses the WebSocket.

### Q-1267. A dropped `push` is warned of only in an inline handler: in a component's fn, as `push(xs, v)` or `xs | push(v)`, and in a top-level fn, check says ok and nothing changes

| Kind | Detected by | Attempts |
|---|---|---|
| silent | quillo-videos-36 making React Quirks #1 (`~/dev/quillo-videos/videos/178-react-state`), on f35bf2d; reproduced for this report | 1 |

Q-505's fix warns of a new value that is dropped, and it does in an inline handler:
`onclick={() => { items.push("eggs") }}` gets "`push` gives a new value and changes nothing, and it is dropped here:
write `items = items | push(...)`". The same statement in a component's own `fn` gets nothing, nor do
`push(items, "eggs")` and `items | push("eggs")`, there or anywhere, nor `xs.push(...)` in a top-level `fn`. Clicked
in Chromium, `G`'s button stays at 1, and `print(add(["milk"]))` prints `["milk"]`.

```quillo
component G() {
    state items = ["milk"]
    fn a() { items.push("eggs") }       // no warning
    fn b() { push(items, "eggs") }      // no warning
    fn c() { items | push("eggs") }     // no warning
    <div><button onclick={a}>a</button><button onclick={b}>b</button><button onclick={c}>c</button>{len(items)}</div>
}

component H() {
    state items = ["milk"]
    <button onclick={() => { items.push("eggs") }}>{len(items)}</button>   // warned
}

fn add(xs) {
    xs.push("eggs")                     // no warning; add(["milk"]) is ["milk"]
    xs
}
```

`repros/Q-1267-dropped-push-in-a-fn.qwe`: one warning, line 16 (`H`), on 0.8.6 (238dac2) and 0.8.8 (6e18cda).

| Impact | |
|---|---|
| **Functionality** | The list never changes; the button keeps showing 1. |
| **User** | A developer from JavaScript, where `push` changes the array, moves the line into a function (as React's handlers often are) and loses the warning that would have told them. |
| **Requests** | No change. |
| **Security** | None. |

**Suggestion:** give the dropped-value warning wherever a statement's value is thrown away, in any function or
handler, and for each of the three spellings of `push` (and the other list and map functions Q-505 covered).

### Q-1268. A plain `let` that a handler changes and the markup shows never updates on the page, and check says ok

| Kind | Detected by | Attempts |
|---|---|---|
| silent | quillo-videos-36 making React Quirks #1, on f35bf2d; reproduced for this report in Chromium | 1 |

A `let` in a component that a handler changes and the markup shows is not a state, so the page never shows the
change, and nothing says so. React's starter flags the React form of this (a ref read during render: oxlint's
"Cannot access refs during render").

```quillo
component Clicks() {
    let clicks = 0
    <button onclick={() => { clicks += 1 }}>{clicks}</button>
}
```

Clicked three times in Chromium: the button shows **0**, on 0.8.6 and 0.8.8 (6e18cda). `quillo check`:
`ok (1 route, 0 functions, 1 component)` (`repros/Q-1268-plain-let-changed-by-a-handler.qwe`).

| Impact | |
|---|---|
| **Functionality** | The count changes in the handler, never on the page. |
| **User** | A button that seems dead; a developer from React (`let` for a variable, `useState` for state) writes it on the first day. |
| **Requests** | No change. |
| **Security** | None. |

**Suggestion:** a warning in `quillo check`, as for the dropped `push`: "`clicks` is changed in a handler but isn't a
state, so the page won't show it: write `state clicks = 0`".

### Q-1269. A form's reset empties a field bound with `value={name}`, but the state keeps the old text: the page and the field disagree

| Kind | Detected by | Attempts |
|---|---|---|
| silent | quillo-videos-36 making React Quirks #1, on f35bf2d; reproduced for this report in Chrome and Chromium | 1 |

The guide says a form's reset brings the fields a component made back to their first values (Q-768's fix), and it
does: the field empties. But a two-way `value={name}` doesn't follow the reset, so the state keeps what was typed,
and the page goes on showing it next to the empty field. (React 19.3 keeps "Ana" in the field: consistent, though
the reset does nothing.)

```quillo
component Reset() {
    state name = ""
    <form><input value={name}/><button type="reset">Clear</button><p>Hello, {name}</p></form>
}
```

Type "Ana", click Clear: the input is `""` and the page still says **"Hello, Ana"**, on 0.8.6 and 0.8.8 (6e18cda), in
Playwright's Chromium (typing) and in Chrome (`repros/Q-1269-form-reset-leaves-the-state`: `typed: text "Hello,
Ana"; after Clear: input "", text "Hello, Ana"`).

| Impact | |
|---|---|
| **Functionality** | The state no longer matches its field; a form sent next carries the old value. |
| **User** | The page says one thing and the field another; a "Clear" button half works. |
| **Requests** | A submit after the reset sends the text the person cleared, when it is sent from the state. |
| **Security** | None. |

**Suggestion:** have two-way binding follow the form's `reset` event (set each bound state to the field's value
after the reset), or say in the guide that it doesn't, and to set the state in `onreset`.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Took Q-1264 as stated (the guide's examples leak) | it came with numbers | measured: the guide's forms don't split; the lesson's own null check first did | 2 timing runs |
| A repro script with `rm -f lib.db*` in zsh | habit | zsh stops on a glob that matches nothing; the script ran in bash after | 1 run |
| Checking the effect repro only with `quillo check` | the finding was about check | ran the page too, for the 500 and its log line | 0 |
| Measured video 175's restart at `localhost` only | the guide's examples use it | the pages were on the WebSocket; at `127.0.0.1` (WebTransport) the same restart took 10 to 23 s (Q-1265, Q-1266) | 3 runs |

## Limitations

None found beyond the issues.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `vacuum into` while the server runs (3 loans) | a whole backup: opened alone, it has the 3 loans |
| A route's `etag`, asked again with `If-None-Match` | a 304, as the guide says (only its log and count are wrong, Q-1262) |
| The effect error at run time | names the function, its effect and the route (`stamp` needs the time effect, ...) |
| `quillo check` on a route `with db` that calls a `fn ... with env` itself | an error naming the effect and the route, as it should (Q-1261 is only the component path) |
| A restart under two tabs, over the WebSocket (video 175, 5 rounds) | both tabs back 1.2 to 1.7 s after the start, and a line typed while the server was down delivered once back, every round |
| A program posting to a channel through a route (`POST /say` with `channel.broadcast`), by curl | a 204, and the line in every open page |
