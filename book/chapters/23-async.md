# No fetch, no async/await {#async}

To save a note from a page, a JavaScript app needs a route on the server, a `fetch` in
the browser, JSON both ways, `async` on every function in between, an `await` at every
call, and a `try`/`catch` for when the network fails. In Quillo you call a function. The
server runs it; the page waits for the answer without blocking; a failure is a value you
can look at. There is nothing else to write, so there is nothing to forget.

## Before and after

The JavaScript way, in its usual shape:

```js
// server
app.post("/api/notes", async (req, res) => {
  const text = req.body.text;
  if (!text?.trim()) return res.status(400).json({ error: "A note needs some text" });
  await db.run("insert into notes (text) values (?)", text);
  res.json(await db.all("select * from notes"));
});

// browser
async function addNote(text) {
  try {
    const res = await fetch("/api/notes", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ text }),
    });
    const data = await res.json();
    if (!res.ok) { showProblem(data.error); return; }
    renderNotes(data);
  } catch (e) {
    showProblem("Could not reach the server");
  }
}
```

The same in Quillo:

```quillo
db.open("app.db")
db.exec("create table if not exists notes (id integer primary key, text text not null)")

server fn load_notes() -> list with db = db.query("select * from notes order by id")

server fn add_note(text: str) -> list ! str with db {
    if trim(text) == "" { return err("A note needs some text") }
    db.exec("insert into notes (text) values (?)", text)
    db.query("select * from notes order by id")
}

component Notes() {
    state notes = load_notes()          // the server runs it while rendering
    state draft = ""
    state problem = null
    <form @submit={() => {
        let r = add_note(draft)          // a call to the server; the page waits for it
        if is_err(r) { problem = r.error } else { notes = r; draft = ""; problem = null }
    }}>
        <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
        <input value={draft}/>
        <p>{problem}</p>
    </form>
}

route GET / = <html><body><Notes/></body></html>
```

That is the whole program. The page arrives with the notes already in its list; Enter in
the field adds one and the list shows it, and a blank note shows `A note needs some text`
under the field, with nothing written.

No API route, no URL, no JSON, no `async`, no `await`, no `try`. `add_note` is checked
like any function: its name, the number and types of its arguments, and the result,
which `quillo check` knows may be an error.

## Calling the server

A `server fn` can be called from a component in two places:

- **In an event handler**, as above. The handler waits for the answer and goes on with
  it, and the page stays responsive meanwhile.
- **To set state**: `state notes = load_notes()`. The server runs it while it renders
  the page, so the page arrives with the data in its HTML; there is no loading spinner
  for the first view and no request after it.

Calling one while the markup is being worked out is a mistake, and `quillo check` says
where it belongs:

```
error: `load` waits on the server, so <Bad> can only call it from an event handler (@click={() => { ... }}) or to set state: state x = load(...)
```

Functions that call the server need nothing either. A helper that calls `add_note` twice
is written as if the calls simply returned; the compiler awaits them, and every function
that calls that helper, all the way up:

```quillo
fn add_twice(text: str) {
    add_note(text)
    add_note(text + " again")
}
```

## Errors are values

A call that cannot be made (the connection drops, the server is down, the server fails)
gives an error value instead of throwing, as `http.*` does on the server:

```
server fn `add_note` could not be reached: Failed to fetch
```

So the same tools handle every failure, the network's and your own:

```quillo
let more = load_more() ?? []                     // carry on with nothing, try again later
let r = add_note(draft)
if is_err(r) { problem = r.error }               // show what went wrong
```

A `server fn`'s own errors (`err("A note needs some text")`) arrive the same way, with
the payload it gave, so the page tells the two apart only if it wants to. A call the
server refused says its status: ``server fn `add_note` failed (403): ...``. A `reply 401
"Log in first"` inside a `server fn` reaches the page as its status alone, since its body
is plain text: return `err("Log in first")` for the page to show the words.

There is no loading flag and nothing done for you on a failure (no retry, no redirect to
a sign-in page): set a `state busy` around the call, and decide what an error shows. A
state set by a `server fn` while the server renders arrives with the page; on a page
reached by `navigate`, it is `null` until the answer comes.

## One after another, or all at once

Calls run in the order they are written, each waiting for the one before. To run several
at once, give them to `parallel`, which gives their results in order:

```quillo
let [user, orders] = parallel(() => load_user(id), () => load_orders(id))
```

In the browser that is `Promise.all`: two calls of 300 ms each take about 300 ms
together instead of 600. On the server each function runs as a coroutine of its own on
the event loop, so their waits (on the database, `http.*`, `sleep`) overlap the same way.
Their computing does not: Quillo code runs one piece at a time, so `parallel` shortens
waiting, not arithmetic. Inside a transaction they run one after another, since the
others' SQL would not be part of it. A function that fails stops the whole call; error
values are results like any other.

## On the server, too

Server code waits the same way. A database query, an `http.get` to another service or a
`sleep` simply waits, and the server answers other requests meanwhile: one thread runs
an event loop, and each request is a coroutine that is set aside while it waits.

```quillo
fn avatar(url: str) with http = http.get(url).body

route GET /profile/:id(int) = {
    user: db.one("select * from users where id = ?", id),
    avatar: avatar(`https://avatars.example.com/${id}`),
}
```

There are no callbacks and no promises to chain. Updating shared state is safe without
locks, too, since only one request runs its code at a time.

The other side of that: code that computes for long without waiting (a loop over a
million rows) keeps the other requests waiting until it ends. Images, password hashing,
files and Rust functions run off the loop; put long computing in Rust, or run more
instances. See [Concurrency](#concurrency-and-reloading), which also says how long
each kind of wait may last.

## Streams

A stream is just as plain. `receive(url, f)` in a handler sends a request and calls `f`
with each part of the answer as it arrives; `listen(url, f)` follows a stream for as
long as the component's code that started it stands. No reader loop, no decoder, no
`await` in a `while`. See [Streaming](#streaming).

## Under the hood

Each call is `POST /_quillo/rpc/<name>` with the arguments as a JSON array (as multipart,
when one of them is a file from an input). They are
checked against the parameter types before the function runs (a 400 otherwise), and the
answer is `{"ok": value}` or `{"err": payload}`. Only `server fn`s can be called, only
with `content-type: application/json` (or that multipart, marked `x-quillo-rpc: 1`), and only from the program's own pages: a request
a browser marks as coming from another site gets a 403. Inside one, `request` is the
current request, and `reply` becomes the HTTP response, so the usual middleware works.

That makes each `server fn` a public address: anyone can call it, with any arguments,
from a script or the browser's console. Check who is asking and what they may touch in
each one, give its parameters types, and return only the columns the page shows. See
[What the browser can reach](#what-the-browser-can-reach).

From hand-written JavaScript, a page's `server fn`s are under `quillo.server`, and
there, being JavaScript, they are awaited:

```js
const notes = await quillo.server.add_note("from a script");
// An error value does not reject: it resolves to { error: ... }.
```

`quillo` is there once the page's components have loaded, which is when someone reaches
for one. A script that runs as the page loads puts `<script src="/_quillo/quillo.js">`
before itself.

See [Server functions](#server-functions) for the rest, and
[Validation](#validation) for checking what a call sends.
