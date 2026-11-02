# Headers, cookies and sessions {#headers-cookies-sessions}

A request brings headers and cookies; an answer carries headers and may set cookies; a
session is a cookie the server signs so that the browser can hold it but not change it.
This chapter shows each of them with a working example: reading what the browser sent,
deciding by it, setting what goes back, keeping a visitor signed in, and ending that.
Everything here is one program; put the parts in one file to try it.

## Reading request headers

`request.headers` is a map of the request's headers, names in lower case. A header
sent twice keeps the last. It is read in a route or a `server fn`; a helper gets
`request` as an argument.

```quillo
route GET /whoami {
    let lang = request.headers["accept-language"] ?? "en"
    {
        host: request.headers.host,
        language: trim(first(split(lang, ","))),
        agent: request.headers["user-agent"],
        ip: request.ip,
    }
}
```

- Names with a `-` are read as `request.headers["user-agent"]`; the others as fields,
  `request.headers.host`.
- Values are text. A pattern (next section) converts them, as it does query values.
- `request.ip` is the TCP peer. Behind a proxy that is the proxy, and Quillo never
  reads `X-Forwarded-For` by itself, since anyone can send one. When the proxy is the
  only way in, read it yourself:

```quillo
fn visitor(req: map) = trim(first(split(req.headers["x-forwarded-for"] ?? req.ip, ",")))

route GET /ip = visitor(request)
```

- An `x-request-id` the proxy sends is kept and echoed on the answer; without one,
  Quillo makes one.
- A request head over 64 KiB, or with more than 100 headers, is answered 431 before any
  route runs.

## Choosing a route by its headers

A route line can carry a request pattern. The route matches only when the headers fit;
otherwise matching goes on to the next route, so a last route can answer the rest:

```quillo
route GET /api/users { headers: { host: "api.example.com" } } = all_users()

route POST /hooks { headers: { "x-hub-signature-256": str } } {
    let sig = request.headers["x-hub-signature-256"]
    if !crypto.equal(sig, `sha256=${crypto.hmac(env("HOOK_SECRET"), request.body)}`) { reply 401 { error: "bad signature" } }
    handle_hook(request.json)
    { ok: true }
}
route POST /hooks = reply 401 { error: "no signature" }
```

A pattern on the host is the way to serve several hosts from one program; for one
database per host, see [Many tenants](#many-tenants).

## Header checks as middleware

A check is a function that gives the request back or ends it with `reply`. It goes
in a route as a pipeline stage, or before a whole set of routes as the middleware of a
`routes` group, which also gives them a shared prefix (see [Route
groups](#routes-route-groups)). `reply` ends the request from anywhere, a helper
included.

```quillo
let api = {
    key: (req) =>
        req.headers.authorization == `Bearer ${env("API_TOKEN")}`
            ? req
            : reply 401 { error: "unauthorized" },
    json: (req) =>
        starts_with(req.headers["content-type"] ?? "", "application/json")
            ? req
            : reply 415 { error: "send JSON" },
}

route GET /api/private = request | api.key | (r) => { ok: true, from: r.ip }
route POST /api/items = request | api.key | api.json | (r) => add_item(r.json)
```

The same checks once, for every route under `/api`:

```quillo
routes /api | api.key {
    route GET /private = { ok: true, from: request.ip }
    routes /items | api.json {                       // groups nest: one more check for these
        route POST / = add_item(request.json)
    }
}
```

For a page rather than an API, the check redirects instead: a `reply` with a status and
a `redirect` goes to the login page from inside the helper (see [Requiring a
session](#sessions)).

## Setting response headers

A route sets headers by returning `respond(status, body, headers)`. Streams and events
take theirs in their options.

```quillo
route GET /api/stats = respond(200, site_stats(), { "cache-control": "public, max-age=60" })

route GET /export.csv = respond(200, csv_text(), {
    "content-type": "text/csv; charset=utf-8",
    "content-disposition": "attachment; filename=\"export.csv\"",
})

route GET /log = stream((out) => { for line in tail_log() { out(line) } },
    { type: "text/plain", headers: { "cache-control": "no-store" } })
```

- Header names are lower case. A name or value with a line break is a runtime error,
  so text from a request cannot add headers of its own.
- `content-length`, `transfer-encoding`, `connection` and `date` are the server's,
  whatever is given. `server: Quillo` is sent unless the route sets `server`;
  `x-request-id` and `x-content-type-options: nosniff` are always added.
- Setting `content-encoding` turns off Quillo's gzip for that answer.
- A plain value (html, a map, a str) gets the default headers and no way to add one:
  switch that route to `respond` when it needs a header.
- A helper cannot add a header: only what the final `respond`, `stream` or `events` is
  given goes out. The exception is cookies, below.

`reply status respond(...)` keeps the headers and body of the `respond` with the new
status, so a helper can answer with headers too:

```quillo
fn open_for_business() {
    if env("MAINTENANCE") == "1" {
        reply 503 respond(503, "Back in a minute", { "retry-after": "60" })
    }
}

route GET /checkout { open_for_business(); <CheckoutPage/> }
```

**CORS.** Browsers ask with an `OPTIONS` request before a cross-site call. Quillo
answers an `OPTIONS` on a path a route has with `204` and `Allow`, but writes no
`access-control-*` header, so the browser refuses the call. Declare the route and send
the headers on the answers too:

```quillo
let CORS = {
    "access-control-allow-origin": "https://app.example.com",
    "access-control-allow-methods": "GET, POST",
    "access-control-allow-headers": "content-type, authorization",
}

route OPTIONS /api/*rest = respond(204, null, CORS)
route GET /api/items = respond(200, items(), CORS)
```

**Content Security Policy.** A header on the page's route, or set by the proxy in front,
decides where the page may load from:

```quillo
route GET /app = respond(200, <Page><Home/></Page>, { "content-security-policy": "default-src 'self'; img-src 'self' data:" })
```

## Cookies

`set_cookie(name, value, options)` adds a `Set-Cookie` to the answer, whatever the route
returns, from anywhere in the request. `cookie(name)` reads one the browser sent, or
null; `request.cookies` is all of them.

```quillo
route POST /theme { form: { theme: str } } {
    set_cookie("theme", theme, { max_age: "1y" })
    redirect("/")
}

route GET / = <Page theme={cookie("theme") ?? "light"}><Home/></Page>

route POST /theme/reset { delete_cookie("theme"); redirect("/") }
```

The options, and what a cookie is without them:

| option | default | takes |
|---|---|---|
| `max_age` | none: the cookie lasts until the browser closes | seconds, or a duration: `"30d"` with `s`, `m`, `h`, `d`, `w`, `y` |
| `path` | `/` (under `--base` or in a mounted program, that prefix) | a path starting with `/` |
| `domain` | the host only | a host name, letters, digits, `.` and `-` |
| `http_only` | `true`: the page's scripts cannot read it | `false` to let them |
| `secure` | `true`: sent over HTTPS only | `false` |
| `same_site` | `"Lax"` | `"Lax"`, `"Strict"` or `"None"` |

- Values are percent-encoded on the way out and decoded when read, so any text fits.
  Browsers hold about 4 KB per cookie.
- `same_site: "None"` makes the cookie `Secure` whatever was asked: browsers take it
  no other way.
- `delete_cookie(name, { path, domain })` must name the path and domain the cookie was
  set with, or the browser keeps it.
- A cookie set twice in one request is sent once, the last.
- `cookie(name)` works in any function the request runs, so a helper that reads the
  visitor's time zone from a cookie need not be handed `request`. Outside a request (a
  job, a script) it is null.
- `Secure` is left out with `--watch`, in desktop apps and with `--plain-http`, where
  the site is served over plain HTTP. In production a browser drops a `Secure` cookie
  that comes over plain HTTP from anywhere but `localhost`, so a site reached as
  `http://10.0.0.5:8080` cannot keep anyone signed in; the server says so once when it
  happens. Serve the site over HTTPS (a proxy in front, sending
  `x-forwarded-proto: https`).

A cookie the page's script reads, for a preference the browser applies before the page
is rendered again:

```quillo
route POST /tz { form: { tz: str } } {
    set_cookie("tz", tz, { max_age: "1y", http_only: false })
    { ok: true }
}
```

Keep secrets out of plain cookies: the browser, and anything on the machine, can read
them. For "remember me", store a random token in the database and put only the token in
the cookie:

```quillo
route POST /remember {
    let id = session().user ?? reply 401 "Log in first"
    let token = crypto.token()
    db.exec("insert into remember (token, user_id, expires) values (?, ?, ?)", token, id, now() + 90 * 24 * 60 * 60 * 1000)
    set_cookie("remember", token, { max_age: "90d", same_site: "Strict" })
    { ok: true }
}

fn user_from_remember() {
    let token = cookie("remember") ?? return null
    let row = db.one("select user_id from remember where token = ? and expires > ?", token, now())
    row.user_id
}
```

## Sessions

A session is a map the server keeps for a visitor, in a cookie it signs with
HMAC-SHA256. The browser holds it and sends it back; it can read it, so keep secrets
out, but it cannot change it, since the signature would no longer match.

```quillo
route POST /login { form: { email: str, password: str } } {
    let user = db.one("select * from users where email = ?", email)
    let hash = user.password_hash ?? crypto.hash_password("not anyone's")   // same work, user or not
    if user == null || !crypto.verify_password(password, hash) { reply 401 "Wrong email or password" }
    set_session({ user: user.id })
    redirect("/")
}

route GET /me {
    let id = session().user ?? reply 401 "Log in first"
    db.one("select id, name from users where id = ?", id)
}

route POST /logout { end_session(); redirect("/") }
```

- `set_session(map, { max_age })` replaces the session. It lasts 30 days unless
  `max_age` says otherwise, in seconds or a duration (`"12h"`).
- `session()` gives the map, or `{}` when there is none, in any function the request
  runs: a route, a `server fn` a component calls, a helper, a pipeline stage. Outside a
  request it is `{}`.
- `end_session()` and `set_session(null)` forget it: the cookie is removed from the
  browser.
- The cookie holds the whole session as JSON, with its expiry, signed; one over about
  3,900 bytes signed is a runtime error. Keep an id in the session and the rest in the
  database.

**Requiring a session.** One helper, used by every page that needs a visitor. `reply`
with a status and a `redirect` ends the request from inside it:

```quillo
fn me() = session().user == null ? null : db.one("select id, name from users where id = ?", session().user)

fn me_or_login(back: str) = me() ?? reply 303 redirect(`/login?next=${url_encode(back)}`)

route GET /account = <AccountPage me={me_or_login("/account")}/>
route GET /orders = <OrdersPage me={me_or_login("/orders")}/>
```

For a `server fn`, which a component calls from the browser, answer with an error value
the page can show instead of a redirect:

```quillo
server fn save_note(text: str) {
    let id = session().user ?? return err("Sign in first")
    db.exec("insert into notes (user_id, text) values (?, ?)", id, text)
}
```

**Signing in from a component.** A `server fn` can call `set_session` too: the cookie
is set on the answer to the call, and the pages the visitor opens next carry it.

```quillo
server fn sign_in(email: str, password: str) {
    let user = db.one("select * from users where email = ?", email)
    let hash = user.password_hash ?? crypto.hash_password("not anyone's")
    if user == null || !crypto.verify_password(password, hash) { return err("Wrong email or password") }
    set_session({ user: user.id })
    { name: user.name }
}

component LoginForm() {
    state email = ""
    state password = ""
    state problem = null
    <form @submit={() => {
        let r = sign_in(email, password)
        if is_err(r) { problem = r.error } else { navigate("/") }
    }}>
        <input name="email" value={email}/>
        <input name="password" type="password" value={password}/>
        <p.hint>{problem}</p>
        <button>Sign in</button>
    </form>
}
```

**How long it lasts, and renewing.** A session ends when its time is up, however often
the visitor comes back: `session()` reads the cookie and never sends it again. For a
session that lasts while it is used, call `set_session` again on the requests that
count as use. Once a day is enough, and keeps the cookie from being sent with every
answer:

```quillo
fn touch() {
    let s = session()
    let day = 24 * 60 * 60 * 1000                      // now() is milliseconds
    if s.user != null && now() - (s.at ?? 0) > day {
        set_session({ ...s, at: now() }, { max_age: "30d" })
    }
}

route GET /dashboard { touch(); <Dashboard me={me_or_login("/dashboard")}/> }
```

**Signing out everywhere.** `end_session()` removes the cookie from one browser; a copy
someone kept stays valid until it expires. To end sessions on the server (a stolen
laptop, a password change), keep a session id in the session and check it against the
database:

```quillo
route POST /login/anywhere { form: { email: str, password: str } } {
    let user = check_password(email, password) ?? reply 401 "Wrong email or password"
    let sid = crypto.token()
    db.exec("insert into sessions (id, user_id, created) values (?, ?, ?)", sid, user.id, now())
    set_session({ user: user.id, sid })
    redirect("/")
}

fn me_checked() {
    let s = session()
    if s.sid == null { return null }
    db.one("select u.id, u.name from sessions s join users u on u.id = s.user_id where s.id = ?", s.sid)
}

route POST /logout/everywhere {
    let id = session().user ?? reply 401 "Log in first"
    db.exec("delete from sessions where user_id = ?", id)
    end_session()
    redirect("/")
}
```

**The key.** Set `QUILLO_SECRET` to a long random string (`crypto.token()` makes one)
and use the same one on every server behind a load balancer. Without it a random key is
made at start, so sessions end when the server restarts. The server warns at start when
the secret is shorter than 32 bytes: a short one can be guessed offline from one signed
cookie.

**The cookie itself.** In production the session cookie is `__Host-quillo_session`:
browsers take a cookie with that prefix only from the very host, over HTTPS, so a
sibling subdomain (another tenant's site) cannot plant one. With `--watch`,
`--plain-http` or in a mounted program it is `quillo_session`. The signature covers the
expiry, the tenant and the program's mount point, so a session is no session past its
time, for another tenant, or in another mounted program. A request with two session
cookies that are both valid here has no session: one of them was planted.

## Putting it together

A small program with a sign-in, a theme cookie, a sliding session and an API behind a
token. Run it, then `curl -c jar -b jar` through the routes.

```quillo
db.open("app.db")
db.migrate("migrations")

let api = {
    key: (req) => req.headers.authorization == `Bearer ${env("API_TOKEN")}` ? req : reply 401 { error: "unauthorized" },
}

fn me() = session().user == null ? null : db.one("select id, name from users where id = ?", session().user)
fn me_or_login(back: str) = me() ?? reply 303 redirect(`/login?next=${url_encode(back)}`)

fn touch() {
    let s = session()
    let day = 24 * 60 * 60 * 1000
    if s.user != null && now() - (s.at ?? 0) > day { set_session({ ...s, at: now() }, { max_age: "30d" }) }
}

view Page(theme: str, children) =
    <html><body class={`theme-${theme}`}>{children}</body></html>

route GET / {
    touch()
    <Page theme={cookie("theme") ?? "light"}>
        {me() != null ? <p>Hello, {me().name}. <a href="/account">Account</a></p> : <a href="/login">Sign in</a>}
    </Page>
}

route GET /login ? { next: str = "/" } =
    <Page theme={cookie("theme") ?? "light"}>
        <form method="post" action="/login">
            <input type="hidden" name="next" value={next}/>
            <input name="email"/> <input name="password" type="password"/>
            <button>Sign in</button>
        </form>
    </Page>

route POST /login { form: { email: str, password: str, next: str = "/" } } {
    let user = db.one("select * from users where email = ?", email)
    let hash = user.password_hash ?? crypto.hash_password("not anyone's")
    if user == null || !crypto.verify_password(password, hash) { reply 401 "Wrong email or password" }
    set_session({ user: user.id, at: now() })
    redirect(starts_with(next, "/") ? next : "/")
}

route GET /account = <Page theme={cookie("theme") ?? "light"}><p>Account of {me_or_login("/account").name}</p></Page>

route POST /theme { form: { theme: str } } { set_cookie("theme", theme, { max_age: "1y" }); redirect("/") }

route POST /logout { end_session(); redirect("/") }

route GET /api/me = request | api.key | (r) => respond(200, { ip: r.ip, user: session().user }, { "cache-control": "no-store" })
```

The `next` a login form carries is checked to start with `/` before it is followed, so a
link from elsewhere cannot send a visitor to another site after signing in. The
`migrations/` folder holds the `users` table; see [Database](#database) for how
migrations run.
