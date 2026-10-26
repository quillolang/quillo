# No fetch, no async/await

To save a note from a page, a JavaScript app needs a route on the server, a `fetch` in
the browser, JSON both ways, `async` on every function in between, an `await` at every
call, and a `try`/`catch` for when the network fails. In Quillo you call a function. The
server runs it; the page waits for the answer without blocking; a failure is a value you
can look at. There is nothing else to write, so there is nothing to forget.

### Before and after

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
        <input bind:value={draft}/>
        <p>{problem}</p>
    </form>
}
```

No route, no URL, no JSON, no `async`, no `await`, no `try`. `add_note` is checked
like any function: its name, the number and types of its arguments, and the result,
which `quillo check` knows may be an error.

### Calling the server

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

### Errors are values

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
the payload it gave, so the page tells the two apart only if it wants to.

### One after another, or all at once

Calls run in the order they are written, each waiting for the one before. To run several
at once, give them to `parallel`, which gives their results in order:

```quillo
let [user, orders] = parallel(() => load_user(id), () => load_orders(id))
```

In the browser that is `Promise.all`: two calls of 300 ms each take about 300 ms
together instead of 600. On the server each function runs on a thread of its own, so
their waits (on the database, `http.*`, `sleep`) overlap the same way; inside a
transaction they run one after another, since another thread's SQL would not be part of
it. A function that fails stops the whole call; error values are results like any other.

### On the server, too

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

### Streams

A stream is just as plain. `receive(url, f)` in a handler sends a request and calls `f`
with each part of the answer as it arrives; `listen(url, f)` follows a stream for as
long as the component's code that started it stands. No reader loop, no decoder, no
`await` in a `while`. See [Streaming](/docs/routes/#streaming).

### Under the hood

Each call is `POST /_quillo/rpc/<name>` with the arguments as a JSON array. They are
checked against the parameter types before the function runs (a 400 otherwise), and the
answer is `{"ok": value}` or `{"err": payload}`. Only `server fn`s can be called, only
with `content-type: application/json`, and only from the program's own pages: a request
a browser marks as coming from another site gets a 403. Inside one, `request` is the
current request, and `reply` becomes the HTTP response, so the usual middleware works.

From hand-written JavaScript, a page's `server fn`s are under `quillo.server`, and
there, being JavaScript, they are awaited:

```js
const notes = await quillo.server.add_note("from a script");
// An error value does not reject: it resolves to { error: ... }.
```

See [Server functions](/docs/components/#server-functions) for the rest, and
[Validation](/docs/validation/) for checking what a call sends.
