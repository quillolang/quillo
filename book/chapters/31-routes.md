# Routes

```quillo
route METHOD /path/:param/:typed(int)/*rest [? { query pattern }] [{ request pattern }] = expr
route METHOD /path { ... block ... }
```

- **Methods:** `GET POST PUT PATCH DELETE OPTIONS ANY`. `ANY` takes every method. A
  `HEAD` request is served by the `GET` route: the route runs (what it does, it does),
  and the body is dropped. A `route HEAD` is accepted but never matches. An `OPTIONS`
  request reaches a route declared `OPTIONS` (or `ANY`). On a path that has routes but
  none for `OPTIONS`, it is answered 204 with `Allow`, the methods the path takes (`GET,
  HEAD, OPTIONS, POST`), and so is `OPTIONS *`: a health check that sends it (HAProxy's
  `option httpchk`) finds the server up. Nothing answers CORS preflights by itself, and
  no `access-control-*` header is written for you.
- **Path params** become variables. `:id(int)` only matches what parses as an int (also
  `float`, `dec`, `num`, `str`, and `date`: `/day/:d(date)` takes `2026-11-20`, not
  `2026-02-30`). `*rest` (or `*name`) takes the rest of the path; `*`
  alone names it `rest`. The path is split at `/`, empty pieces dropped, so `/users`,
  `/users/` and `//users` are one path; each piece is percent-decoded after the split, so
  a `%2F` stays inside its piece. `request.path` is the path as it came, undecoded.
- **Query pattern:** `? { q: str, page: int = 1 }`. Query values are text, so `int`, `float` and `bool` patterns convert them.
  A name sent more than once keeps its last value in `request.query` and `request.form`;
  a list in the pattern takes them all, as a group of checkboxes sends: `? { tag: [str] }`
  gives `["a", "b"]` for `?tag=a&tag=b`, one value a list of one, and none an empty list.
  So does `{ form: { tag: [str] } }`, for a urlencoded form.
- **Request pattern:** matched against the `request` map. Its fields, and all of them:
  `method`, `path`, `query`, `query_string` (the query as it came), `params`, `headers`,
  `cookies`, `body`, `json`, `form`, `files`, `ip`, `id`. The host is `request.headers.host`; the tenant and the locale
  are `tenant()` and `locale()`. JSON is matched strictly: the string `"7"` is not an
  int. Form fields, headers and cookies are converted like query values. `{ json: User }`
  takes the body as a [record](#records). A pattern on the host is a pattern on the
  headers: `{ headers: { host: "api.example.com" } }`.
- **What the request holds.** `headers` has its names in lower case, and a header sent
  twice keeps the last; only the last `Cookie` header is read, its values
  percent-decoded. `query` and `form` keep the last of a key given twice (`a=1&a=2` is
  `{ a: "2" }`): no lists, and `a[b]` is a key named so. `json` is the body parsed when
  the content type is `application/json` (or `...+json`), or when there is none and the
  body starts with `{` or `[`; otherwise, and for JSON that does not parse, it is
  `null`. `body` is the body as text: bytes that are not UTF-8 are replaced, so a binary
  body cannot be read as it was (there is no bytes value in the language), and for a
  multipart body it is empty. `id` is the request's id (see [Services](#services)). A
  JSON body that starts with a byte-order mark is read.
- **What the server refuses before a route.** Two `Host` headers, an HTTP/1.1 request
  without one, or one that is not a host and a port, get 400; a method not written in
  capitals, or one HTTP does not define (`FROB`), gets 501. A request target in absolute
  form (`GET http://host/path`) takes its host from the target. A `HEAD` answer carries
  the `Content-Length` the `GET` would. A body sent with `Content-Encoding: gzip` reaches
  the route decoded, [`--max-body`](#options-of-quillo-run) (`quillo run main.qwe --max-body 50MB`; 10MB unless
  given) counting what it decodes to (more is 413); one in
  another coding gets 415, and one that is not the gzip it says, 400. An `Expect` other
  than `100-continue` gets 417. An HTTP/1.0 request gets no `100 Continue` (a 1.0 client
  would take it for the answer) and no chunks: a streamed answer goes with no length and
  ends with the connection, and an `Upgrade` is ignored.
- **Reserved:** paths under `/_quillo/` belong to Quillo and never reach a route (what is
  there is listed under [What a route cannot do](#what-a-route-cannot-do)).
- **Order:** routes are tried in the order they are declared, and the first whose method, path and patterns all match handles the request. A later route can be the fallback.
  Routes are declared at the top level of a file: `quillo check` refuses one inside a
  function or a block.

If no route matches, the response is 404 for an unknown path, 405 for a known
path with the wrong method (with `Allow`, the methods it takes), and 400 when a route
matched the path and method but none of its patterns matched. Before any route is tried, a `POST`, `PUT`, `PATCH` or
`DELETE` with a form, multipart or plain-text body (or none) that the browser marks as
coming from another site is refused with 403 (see `allow_cross_site` under
[Standard library](#standard-library)). A last `route ANY /*rest` is a 404 page of
your own; since `ANY` matches every method, no path then answers 405 or 400.

The statuses the server answers by itself:

| Status | When |
|---|---|
| 204 | `OPTIONS` on a path no `OPTIONS` route takes, and `OPTIONS *`, with `Allow` |
| 304 | a file's, or a route's own `etag`, that the browser already holds |
| 400 | a malformed request line, header or chunked body; a body that is not the gzip it says; a route matched the path and method but no pattern; a `server fn` call whose body is not a JSON array; a WebSocket handshake that is not a `GET` with `Upgrade` and `Connection` |
| 403 | a cross-site form post, `server fn` call or live connection; a phone app built the old way, on 127.0.0.1, whose launch token is missing |
| 404 | no route; an unknown `/_quillo/` path; a `file()` path refused |
| 405 | the path is known, the method is not (with `Allow`) |
| 408 | a request head not whole after 30 s, or a body of which nothing came for 30 s (logged: `POST /up HTTP/1.1 -> 408 (nothing of the body came for 30 s)`) |
| 409, 422 | an [idempotent request](#idempotent-requests) running, or sent again with another body |
| 412 | a file whose `If-Match` or `If-Unmodified-Since` it does not meet |
| 413 | a body larger than `--max-body`, sent or once decoded |
| 414 | a request line over 64 KiB |
| 415 | a body in a coding other than gzip; a `server fn` call without `content-type: application/json` |
| 416 | a `Range` past the end of a file |
| 417 | an `Expect` other than `100-continue` |
| 426 | a WebSocket handshake of a version other than 13 |
| 431 | a request head over 64 KiB, or more than 100 headers |
| 500 | a runtime error in the route (the body is `internal error`; the message is on stderr, and on the page under [`--watch`](#options-of-quillo-run)) |
| 501 | a `transfer-encoding` other than `chunked`; a method HTTP does not define |
| 503 | the server stopping or its program failed to load; a prefetch refused; too many files being sent or pages waiting (with `retry-after: 5`) |

## Route groups {#routes-route-groups}

Routes that share a start of their path, middleware or a request pattern are written once
for all of them, in a `routes` group, as Laravel's `Route::prefix()->middleware()->group()`:

```quillo
fn signed_in(r) = session().user != null ? r : reply 303 redirect(`/login?next=${url_encode(r.path)}`)
fn admins(r) = is_admin(session().user) ? r : respond(403, "admins only")

routes /admin | signed_in | admins {
    route GET / = <AdminHome/>                      // GET /admin
    route GET /users = list_users()                 // GET /admin/users
    route POST /users/:id(int)/ban { ban(id) }

    routes /teams/:team(int) {                      // groups nest: /admin/teams/7/members
        route GET /members = members(team)
    }
}

routes /api { headers: { host: "api.example.com" } } {    // Laravel's domain()
    route GET /me = me()
}

routes / | signed_in {                              // no prefix: middleware only
    route GET /settings = <Settings/>
}
```

- **The path** goes before each route's own, parameters and types included
  (`:team(int)`), which the routes' bodies and patterns can use; a group's `/` alone
  adds nothing.
- **Middleware** (each after a `|`, as in a pipeline) is called with `request` before the
  route's body, outer groups' first, in the order written. A `reply` in it answers the
  request in the route's place, as does a response it gives back (`redirect(...)`,
  `respond(...)`); anything else it gives lets the route go on, so the middleware of a
  pipeline (`auth.required`, which gives the request back) works as it is. What it gives
  does not reach the route: a route that needs the user reads it again (`session()`,
  `me()`). `quillo check` checks each call as it checks any other, at the line the
  middleware is written on.
- **A request pattern** after the middleware is matched as well as each route's own
  (both must match), and binds the same way: a request it does not match goes on to the
  next route, as with a route's own pattern.
- A group holds only `route`s and `routes` groups, is written at the top level of a file,
  and keeps its routes in the order written, among the file's others. Outside that,
  `routes` is a name like any other (`routes / 2` divides). Routes from groups are
  routes: `quillo docs` lists them by their whole path.
- **Back where they were.** The login page sends the visitor on to `next` once signed in.
  `next` came in the address, so anyone can write a link whose `next` is another site:
  `local_path(next)` gives `next` when it is an address on this site as a browser reads it
  (tabs and line breaks dropped, `\` read as `/`), else null. `//evil.example`,
  `/\evil.example`, `https://...` and `javascript:...` are null. It works on both sides.

  ```quillo
  route GET /login ? { next: str = "/" } =
      <form method="post" action={`/login?next=${url_encode(next)}`}>   // next goes on with the form
          <input type="email" name="email"/>
          <input type="password" name="password"/>
          <button>Log in</button>
      </form>

  route POST /login ? { next: str = "/" } { form: { email: str, password: str } } {
      sign_in(email, password)?
      redirect(local_path(next) ?? "/")
  }
  ```

  A visitor who is not signed in and opens `/admin/users` is sent to
  `/login?next=%2Fadmin%2Fusers` by `signed_in`; the form posts to that same address, and
  once signed in they are back on `/admin/users`. A link whose `next` is
  `//evil.example` lands on `/`.

## Uploads

A form with `enctype="multipart/form-data"` sends files. Its text fields are in
`request.form`, as for any form, and each file is in `request.files` under its field's
name: `{ field, filename, type, size }`. The shape follows the count: a field that sent
one file holds its record, and a field with several, from `multiple`, a list of them. A
pattern makes it one shape: `{ files: { docs: [map] } }` takes every file of that field
as a list, one file a list of one and none an empty list, as `[str]` does for form fields:

```quillo
route GET /docs =
    <form method="post" action="/docs" enctype="multipart/form-data">
        <input type="text" name="title"/>
        <input type="file" name="docs" accept=".pdf" multiple/>
        <button>Send</button>
    </form>

route POST /docs { files: { docs: [map] } } {
    for d in docs { save_upload(d, `uploads/${uuid()}.pdf`)? }
    redirect("/docs")
}
```

The whole round trip for one file: the page with its form, then the route the form posts
to. The bytes stay with the request until saved:

```quillo
route GET /avatar =
    <form method="post" action="/avatar" enctype="multipart/form-data">
        <label>Your photo <input type="file" name="photo" accept="image/*" required/></label>
        <button>Upload</button>
    </form>

route POST /avatar {
    let f = request.files.photo
    if f == null { reply 400 "Choose a photo" }
    if f.size > 2_000_000 { reply 413 "Photos up to 2 MB" }
    let path = save_upload(f, `uploads/${uuid()}.png`)?   // into the program's folder
    redirect(`/${path}`)
}

route GET /uploads/:name = file(`uploads/${name}`)
```

The form needs `method="post"` and `enctype="multipart/form-data"`: without the enctype,
the browser sends only the file's name. Each file input's `name` is its key in
`request.files`; `accept` and `required` are the browser's own checks, which a visitor
can skip, so the route checks again (`f == null`, `f.size`). A file chosen in a component
goes to a `server fn` instead, without a form (see `save_upload` in the reference).

- `save_upload(file, path)` writes the file under the program's folder (making folders
  as needed) and gives the path back; `read_upload(file)` gives a text file's contents.
  Both give an error value when the request has no files, or the record is not one of
  this request's; a path that leads outside the folder, by `..` or through a link
  (see [Paths and links](#paths-and-links)), is a runtime error. `save_upload` needs the
  `fs` effect.
- `save_upload`, `write_file` and `delete_file` refuse the program's own files, and give
  an error value saying so: the `.qwe` and Rust (`.qwer`, `.rs`) files it loaded (or a folder holding
  one), `.env` and `.env.*`, the SQLite database it has open with its `-wal`, `-shm` and
  `-journal` files, and anything in `migrations/`.
- `filename` is what the browser sent, without any folders; name saved files yourself
  (as above) rather than trusting it. Save under a folder of the program's with a name the
  program makes (`uploads/${uuid()}.pdf`), and keep the visitor's name in the database.
  `quillo check` warns on `save_upload(f, f.filename)`, and on a path that is only a form
  value, then `/`, then the filename. The record also carries `start` and `end`, the
  file's place in the body.
- A file's name and a field's name decode the three escapes HTML's multipart encoding
  writes (`%22` for `"`, `%0D`, `%0A`) and nothing else, and a quoted name keeps a `;` in it.
- A multipart body may hold up to 1,000 parts; a text field given twice keeps the last.
- A request body may be up to 10MB, the files and the form's fields together; a larger one
  is answered 413. For larger files, start the program with a higher limit, the option
  [`--max-body`](#options-of-quillo-run): `quillo run main.qwe --max-body 50MB`.
  A body may take as long as it keeps coming, a photo from a phone on a weak link too; one of
  which nothing comes for 30 s is answered 408, with a line in the log. A proxy in front
  has limits of its own (nginx's `client_body_timeout`). The request's head (its line and headers) has limits of its own, 64 KiB and 100
  headers, answered 431.

From a component, a file goes to a `server fn` as an argument: pass it the input's file,
and the function gets the same `{ field, filename, type, size }` record a form's upload
is, for `save_upload`:

```quillo
server fn add_photo(file: map) = save_upload(file, `photos/${uuid()}.jpg`)

component Photo() {
    state saved = null
    <input type="file" accept="image/*" @change={(e) => { saved = add_photo(e.target.files[0]) }}/>
}
```

A file anywhere in the arguments goes as a file, inside a list or a map too, and a
`FileList` (`e.target.files`) is a list of files, so `add_photos(e.target.files)` gets a
list of records. Such a call is sent as multipart with an `X-Quillo-Rpc` header, which
pages of other sites cannot add without asking first; a form another site posts is refused.

## Checking code from a program

`check_program("main.qwe", { "plugins/x.qwe": text })` gives what `quillo check` says of
a program, as `{ file, line, severity, message }` maps, with some files' text given in
place of what is on disk: a tool that writes Quillo code can check it before writing
it. The code checked may come from users, so it reads only files inside the program's
folder: a `use`, `mount` or `service` path with `..`, starting with `/`, or leading out
of the folder through a link is reported as an error (`path must stay inside the
program's folder: data/x.qwe`), not read. `delete_file(path)` removes a file or folder inside the
program's folder.

## Scripts from users

`run_script(source, vars)` runs code a program's users write (a rule on a form's field,
who may see a record) on the server, and gives its last expression's value, or an error
value saying what went wrong:

```quillo
run_script("value > 3 ? null : `at least 4, got ${value}`", { value: 5 })   // null
```

- The script sees the values in `vars` and the builtins that only compute (`len`,
  `upper`, `map`, `filter`, ...). Method calls are the same: `"x".upper` works,
  `"HOME".env` and `"f".read_file` do not, nor the program's functions.
- It cannot define functions (it can write `(x) => ...`), routes, state or markup,
  `use` files or reply.
- It has no effects of its own: a builtin such as `env` passed in `vars` still cannot
  run. A function of the program passed in `vars` runs as the program's, with what the
  program may do, so it is the program's way to let a script ask for more
  (`{ taken: (code) => db.one(...) != null }`).
- It stops after 20,000 steps (calls and loop rounds). A value it makes may be up to
  1 MB (strings, and lists and maps counted with what they hold), and each KB it makes
  costs a step: doubling a string in a loop stops there, not when the memory runs out.
- Like any code, it may nest at most 256 levels deep.

## Images

```quillo
image.info("photos/a.jpg")              // { width, height, format, alpha }, or an error
image.convert("photos/a.jpg", "cache/a-800.jpg", {
    crop: { x: 100, y: 0, w: 1200, h: 675 },   // in the image's pixels
    width: 800,                                 // shrunk to it, never enlarged
    format: "jpeg", quality: 82,                // jpeg, png or webp (lossless)
})
```

JPEG, PNG, WebP and GIF are read. `image.info` reads only the header, so it is a cheap
check that an upload is an image at all. Decoding is limited to 12,000 pixels a side
and 512 MiB, so a small file claiming to be enormous is refused before anything is
allocated; both run off the event loop. At most four conversions run at once (fewer on
a machine with fewer cores), so many uploads together cannot each take 512 MiB; the rest
wait their turn, and past 64 waiting `image.convert` gives an error value at once. Paths
stay inside the program's folder.

An upload made into a picture for the page: the form, the route that keeps the file and
makes an 800-pixel copy, and the page that shows it:

```quillo
route GET / =
    <form method="post" action="/photos" enctype="multipart/form-data">
        <input type="file" name="photo" accept="image/*" required/>
        <button>Upload</button>
    </form>

route POST /photos {
    let f = request.files.photo ?? reply 400 "Choose a photo"
    let id = uuid()
    let original = save_upload(f, `uploads/${id}`)?
    let info = image.info(original)
    if is_err(info) { delete_file(original); reply 415 "That file is not a JPEG, PNG, WebP or GIF image" }
    image.convert(original, `thumbs/${id}.jpg`, { width: 800, format: "jpeg", quality: 82 })?
    redirect(`/photos/${id}`)
}

route GET /photos/:id = <figure>
    <img src={`/thumbs/${id}.jpg`} width="800"/>
    <figcaption>{image.info(`uploads/${id}`).width} pixels wide, shown at 800</figcaption>
</figure>

route GET /thumbs/:name = file(`thumbs/${name}`, { cache: "1y" })
```

A photo 1,600 pixels wide goes to its page, `1600 pixels wide, shown at 800`, with the
copy served as `image/jpeg`. A text file named `.jpg` is not kept, and the answer is
`That file is not a JPEG, PNG, WebP or GIF image` (415).

## Cookies and sessions

```quillo
route POST /login { form: { email: str, password: str } } {
    // stored the same way at signup: one Unicode form, trimmed, in lower case
    let user = db.one("select * from users where email = ?", lower(normalize(trim(email))))
    if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }
    set_session({ user: user.id })
    redirect("/")
}

route GET /me {
    let id = session().user ?? reply 401 "Log in first"
    db.one("select name from users where id = ?", id)
}

route POST /logout { end_session(); redirect("/") }
```

The pages that call them: a form that posts the email and password, and a logout button,
which is a form too, since `/logout` takes a `POST` (a link would be a `GET`, which a
prefetch could follow):

```quillo
route GET /login =
    <html><body>
        <form method="post" action="/login">
            <label>Email <input type="email" name="email" required/></label>
            <label>Password <input type="password" name="password" required/></label>
            <button>Log in</button>
        </form>
    </body></html>

route GET / = <html><body>
    {session().user == null
        ? <a href="/login">Log in</a>
        : <form method="post" action="/logout"><button>Log out</button></form>}
</body></html>
```

A wrong password, or an email no one has (`user` is null, so its `password_hash` is null,
which `verify_password` says is false), shows the plain text `Wrong email or password`
with status 401. The right one sets the cookie and goes to `/`, which now shows the
button; `/me` then gives `{"name":"Ana"}`, and after logging out `Log in first` (401).

- `set_session(map, { max_age })` replaces the session (30 days unless given),
  `session()` gives it (`{}` when there is none) and `end_session()` forgets it.
  `max_age` is seconds or a duration (`"12h"`, as for cookies below). A session keeps
  decimals, and ints past 2^53, as they are, so `session().total + 1m` adds.
- A session ends when its time is up, however often the visitor comes back: `session()`
  reads the cookie and never sends it again. For a session that lasts while it is used,
  call `set_session` again on the requests that count as use. Once a day is enough, and
  keeps the cookie from being sent with every answer:

```quillo
fn touch() {
    let s = session()
    let day = 24 * 60 * 60 * 1000                      // now() is milliseconds
    if s.user != null && now() - (s.at ?? 0) > day {
        set_session({ ...s, at: now() }, { max_age: "30d" })
    }
}
```
- A session is a cookie signed with HMAC-SHA256: the browser can read it but not
  change it, so keep secrets out of it. Cookies hold about 4 KB; keep an id in
  the session and the rest in the database. The signature covers its expiry, its tenant
  and the program's mount point, so it is no session past its time, for another
  tenant, or in another mounted program.
- `end_session()` removes the cookie from the browser, but a copy someone kept stays
  valid until it expires. To end sessions on the server (log out everywhere, a stolen
  laptop), keep an id in the session and check it against the database.
- The session cookie is `__Host-quillo_session` when it is `Secure` and for the whole
  site (as in production): browsers take a cookie with that prefix only from the very
  host, over HTTPS, so a sibling subdomain (another tenant's site) cannot plant one.
  Elsewhere ([`--watch`](#options-of-quillo-run), `quillo run main.qwe --watch` while working on a program; a
  mounted program) it is `quillo_session`. A request with two
  session cookies that are both valid here has no session: one of them was planted.
- Set `QUILLO_SECRET` to a long random string (`crypto.token()` makes one) and use the
  same one on every server. Without it, a random key is made at start: sessions end
  when the server stops. The server warns when it starts if the secret is shorter than
  32 bytes, as it does for `QUILLO_SERVICE_SECRET` and `LIMER_ADMIN_SECRET`: a short one
  can be guessed offline from one signed cookie.
- `set_cookie(name, value, { max_age, path, domain, http_only, secure, same_site })` and
  `delete_cookie(name, { path, domain })` add `Set-Cookie` to the response, whatever
  the route returns. Cookies are `HttpOnly`, `SameSite=Lax`, `Path=/` and `Secure` by
  default. `max_age` is seconds or a duration (`"30d"`: `s`, `m`, `h`, `d`, `w`, `y`);
  `path` must start with `/`; `domain` takes letters, digits, `.` and `-`; `same_site`
  is `"Lax"`, `"Strict"` or `"None"`, and `"None"` makes the cookie `Secure` whatever
  was asked, since browsers take it no other way. Values are percent-encoded. A cookie
  set twice in one request is sent once, the last. In a mounted program, or under
  [`--base`](#options-of-quillo-run), the default `Path` is the prefix.
  `Secure` is left out with `--watch` and in desktop apps, which are served
  over plain HTTP on this machine; in production, serve the site over HTTPS (a proxy
  such as limer in front). A browser drops a `Secure` cookie that comes over plain HTTP
  from anywhere but `localhost`, so a site reached as `http://10.0.0.5:8080` cannot keep
  anyone signed in: the server says so once, naming the cookie, the first time it sends
  such a cookie to a request that came straight from another machine (not through a proxy
  here, and without `x-forwarded-proto: https`). On a network you trust, start the program with
  [`--plain-http`](#options-of-quillo-run), `quillo run main.qwe --plain-http`, which leaves `Secure` out, as
  `--watch` does. `delete_cookie` is `Secure` only when the request
  came over HTTPS (or a proxy's `X-Forwarded-Proto: https` says so), when the name needs it
  (`__Host-`, `__Secure-`), or when `secure: true` is given, so a cookie set with `secure:
  false` can be removed over plain HTTP. `request.cookies` has what the browser sent, and
  `cookie(name)` one of them (or null), in any function the request runs, as
  `session()` is: a helper that reads the visitor's time zone from a cookie need not be
  handed `request` by every caller. Outside a request (a job, a script), `session()` is
  `{}` and `cookie(name)` is null.
- `set_session` takes only `max_age`; `set_session(null)` ends the session too. The
  cookie's value is the session as JSON, with its expiry, base64url-encoded and signed,
  so a session over about 3,900 bytes is a runtime error.

## What a route returns

| Value | Response |
|---|---|
| html | 200 `text/html`; XML when its root names a namespace (`<urlset xmlns="...">`, a sitemap) or it starts `<?xml`: `application/xml`, `application/rss+xml` for `<rss>`, `application/atom+xml` for `<feed xmlns="...">`, and `image/svg+xml` for `<svg xmlns="...">` |
| str | 200 `text/plain` |
| map, list, number, bool | 200 JSON |
| null | 204 |
| error | 500 `{"error": ...}` |
| `respond(status, body, headers)` | exactly that |
| `redirect(url, status = 303)` | a redirect |
| `file(path, { cache })` | a file under the program's folder (see [Static files](#static-files)) |
| `files(folder, path, options)` | a folder of files, as a static host serves it (see [Static files](#static-files)) |
| `markdown_page(text, { title, lang, html })` | Markdown as a whole page: a doctype, a `<head>` with the title (the option's, else the first `#` heading), and the HTML |
| `stream(f, { type, status, headers })` | sent as `f` makes it (see [Streaming](#streaming)) |
| `events(f, { status, headers })` | Server-Sent Events, as `f` sends them |
| a runtime error | 500 `text/plain`, `internal error` |

- An error the program wrote, `err("That name is taken")`, is sent as it is. One a builtin
  made says `internal error` instead, wherever it would reach a visitor: as a route's
  value, inside a map or list, in markup, or from a `server fn`, and when it is turned
  into text: a template string, `str`, `join`, `json.stringify`. A database's has the SQL
  and the tables, an `http` call's the address with its key, a file's its path, so that
  text goes to the log with the request. `print` shows it too, as [`--watch`](#options-of-quillo-run)
  (`quillo run main.qwe --watch`) and a script
  do, and code on the server still reads it: `db.error_kind(r)`, `is_err(r)`, `error(r)`.
  What is read that way is plain text, no longer protected: `{ line: error(r) }` sends
  it. To show a visitor something, say it:
  `err("Billing is down, try again soon")`. A service's own `err` reaches its caller's
  visitors in the same way.
- A failed request's line in the log names it, with the id in its `x-request-id`:
  `quillo: error: GET /checkout (rid 1cf61931): main.qwe:8: division by zero`. Under
  [`--log json`](#options-of-quillo-run) it is an object, with `level`, `request` and `error`.
- `respond(status, body, headers)` takes a status from 200 to 599. A 1xx is an interim
  answer, after which the client waits for the real one, so it is refused, when the
  program runs and in `quillo check` for a written one (`` `respond` status must be
  200-599, got 103: a 1xx is an interim answer, and the client would wait for the real
  one ``); `reply` takes 200 to 599 too. A 205 goes with no body, whatever is given, and
  each status has its reason phrase (`418 I'm a teapot`). A route's answer that carries
  an `etag` is a 304 to a `GET` or `HEAD` whose `If-None-Match` holds it, as files are:
  `` respond(200, v, { etag: `"${version}"` }) ``. Header names are written in lower case, and a name or value with a control character (a line break) is
  a runtime error. `content-length`, `transfer-encoding`, `connection` and `date` are
  the server's, whatever is given. `redirect(url, status)` takes 300 to 399.
- Every answer carries `server: Quillo` (unless the route sets `server`), `date`,
  `x-request-id` and `x-content-type-options: nosniff`. Every HTML page carries
  `x-frame-options: SAMEORIGIN`, unless the route sets one or a
  `content-security-policy` with `frame-ancestors`. A page made from who the visitor is
  (it called `session()` or `cookie()`, or set a cookie) carries `cache-control:
  private` and `vary: cookie`, unless the route set its own `cache-control`, so a shared
  cache never gives one visitor's page to another.
- `site_headers(map)` at the top level adds headers to every answer that does not set
  them itself, and `site_headers("/admin", map)` to those under a path; a longer path's
  rule wins over a shorter one's. A `null` value takes away one Quillo sends. Names are
  checked, and values are one line; `content-length`, `transfer-encoding`, `connection`,
  `set-cookie` and `content-type` are refused, as a route sets them:

  ```quillo
  site_headers({ "content-security-policy": "default-src 'self'", "referrer-policy": "same-origin" })
  site_headers("/embed", { "x-frame-options": null, "content-security-policy": "frame-ancestors *" })
  ```
- Text answers are gzipped for browsers that accept it: a 200 with a body of 1 KiB or
  more whose type is `text/*` or names `json`, `javascript` or `svg`, with `vary:
  accept-encoding`. `Accept-Encoding` is read as HTTP says: names in any case, `x-gzip`
  as `gzip`, `gzip;q=0` refusing it, and `*` standing for gzip when gzip is not named. Not when the route set `content-encoding`, not streams, not partial
  files (206). There is no brotli.
- A header cannot be added from a helper: what `respond`, `stream` and `events` are given
  is the answer's, and headers for every page are `site_headers`'. Cookies are the
  exception: `set_cookie` anywhere in the request joins it. `reply status respond(...)` keeps the `respond`'s headers and body with the new
  status.

`reply status [body]` ends the request right away, from anywhere: inside a
helper, a pipeline stage or a `??` fallback. This is how middleware works:

```quillo
let auth = {
    required: (req) =>
        req.headers.authorization == `Bearer ${env("TOKEN")}`
            ? req
            : reply 401 { error: "unauthorized" },
}

route GET /me = request | auth.required | (r) => { ip: r.ip }

route GET /users/:id(int) {
    let user = users | find((u) => u.id == id) ?? reply 404 { error: `no user ${id}` }
    user
}
```

## What a route cannot do

The questions that come up once an app is past its first pages, answered from the
server's code:

- **A hook before every route, or a global error handler:** the hook is a `routes /`
  group with middleware around the routes that need it (see [Route groups](#routes-route-groups));
  for one route, a pipeline stage that `reply`s (above). There is no global error
  handler: a runtime error is always the plain `internal error` 500, and a 404 page of
  your own is a last `route ANY /*rest`.
- **`request` in a helper:** `request` is read only in a route or a `server fn`; a
  function they call gets it as an argument. `session()`, `cookie(name)`, `tenant()`
  and `locale()` work in any function the request runs.
- **A response header from a helper:** no; see [What a route returns](#what-a-route-returns).
- **HTTPS, HTTP/2, HTTP/3 for pages:** the server speaks HTTP/1.1 over plain TCP
  (keep-alive, pipelining, chunked bodies and `Expect: 100-continue` included), and
  HTTP/1.0 as 1.0 frames it; a proxy in front terminates TLS. The option [`--tls-cert`](#options-of-quillo-run) is for WebTransport only (see [Channels](#channels)).
- **A WebSocket route of your own:** no. Upgrades are taken only on Quillo's own paths
  (live data, channels, [`--watch`](#options-of-quillo-run)); a route gets a plain request. Pages hear from the
  server through `live`, `events`/`listen`, or channels.
- **Bytes:** there is no bytes value. A body arrives as text (above), a route answers
  text, HTML or JSON, and `stream` sends text or lines of JSON. Files are the way bytes
  go out: `file()` sends one as it is, so write what you made (a PDF, an image) under the
  program's folder and serve it from there. The body is read whole, up to [`--max-body`](#options-of-quillo-run),
  before the route runs: it cannot be read as it arrives.
- **A time limit on a route:** for computing, starting the program with [`--max-compute 2s`](#options-of-quillo-run) (`quillo run main.qwe --max-compute 2s`) ends a request that
  computes that long without letting go, as a 500; see
  [Concurrency](#concurrency-and-reloading). A wait is not limited: a route waiting on
  something that never comes holds its coroutine for good, and no one else. Also limited
  is the socket: a request head not whole after 30 s, or a body of which nothing has
  come for 30 s, is answered 408 and dropped (a body that keeps coming may take as long
  as it takes), an idle keep-alive connection is closed after 60 s, and a client that
  stops reading its answer is let go after 30 s.
- **Rate limiting:** none for routes (a `rate_limit` builtin was removed). Count what
  matters in a table of your own: wrong PINs per account, uploads per hour. What is
  limited by itself: 256 large files being sent at once (8 per client), 4096 pages
  waiting for live data (256 per client), answered 503 with `retry-after: 5`.
- **The visitor behind a proxy:** `request.ip` is the TCP peer, unless [`--trust-proxy`](#options-of-quillo-run)
  names it (see [Live data](#live-data)): `X-Forwarded-For` is read from no one else. An
  `x-request-id` a proxy sends is kept and echoed.
- **Matching the host or a subdomain in the route line:** no; match the `host` header
  in a request pattern, or use `tenant_from` (see [Many tenants](#many-tenants)).
- **Logs anywhere but stderr:** the request log and errors go to stderr, text or JSON
  (see [Logs and metrics](#logs-and-metrics)); a service manager or a container collects
  them. There is no log file or syslog option.
- **Quillo's own addresses.** Under `/_quillo/` the server answers: `health`, `ready`
  and, with [`--metrics`](#options-of-quillo-run), `metrics`; `live` (the live-data WebSocket, or a long poll where
  it cannot open); `wt` and `wt-ticket` (channels); `events`, `dev` and `src/...`
  (`--watch`); `quillo.js`, `media.js`, `b/<hash>.js`, `r/<hash>.js` and
  `s/<hash>.css` (what pages load); `rpc/<name>` (`server fn` calls) and `markdown`;
  `service/<name>` and `join` (services); `announce` ([`--register`](#options-of-quillo-run)); `describe`
  ([`--describe`](#options-of-quillo-run)); `posted/<id>` (in an app, a form's answer, shown once). `job/<name>`, `channel/<name>`, `udp/<port>` and `later` are the
  process's own, for its instances, and are 404 to anyone else. Anything else there is
  404. Under a mount or [`--base`](#options-of-quillo-run), `/shop/_quillo/...` serves the mounted program's
  pages' needs (its bundle, calls, live data) and its `ready`; `health` and `metrics`
  are the process's, at the root.

## Idempotent requests

A client that sent a payment and got no answer (a timeout, a dropped connection) cannot
tell whether it was made. `idempotent(f)` lets it send the request again: a request
with an `Idempotency-Key` header runs once, and the same request again with the same
key gets the first answer, without running again. It works as Stripe's API does
([idempotent requests](https://docs.stripe.com/api/idempotent_requests); the header is
the IETF's [draft](https://datatracker.ietf.org/doc/draft-ietf-httpapi-idempotency-key-header/)):

```quillo
route POST /payments { json: { to: str, amount: dec } } = idempotent(() => {
    let id = pay(to, amount)?
    respond(201, { id })
})

server fn transfer(from: str, to: str, amount: str) =
    idempotent({ isolation: "serializable", retries: 5 }, () => move(from, to, amount))
```

A client of the route sends the key in a header, and the same key again when it retries:

```sh
curl localhost:8080/payments -H "Idempotency-Key: $(uuidgen)" --json '{"to": "bob", "amount": "12.50"}'
```

A page calls the `server fn` as any other; it makes the key and sends it by itself, so
the component has nothing to add for it:

```quillo
component Pay() {
    state sent = null
    <div>
        <button @click={() => { sent = transfer("ana", "bob", "12.50") }}>Send 12.50 to Bob</button>
        {is_err(sent) ? <p>Not sent: {error(sent)}</p> : sent && <p>Sent, transfer {sent.id}</p>}
    </div>
}
```

Sent twice with one key, the second answer is the first, with `Idempotent-Replayed: true`,
and `move` has run once:

```sh
curl localhost:8080/_quillo/rpc/transfer -H "Idempotency-Key: k-77" --json '["ana", "bob", "12.50"]'
# {"ok":{"id":1}}
curl -i localhost:8080/_quillo/rpc/transfer -H "Idempotency-Key: k-77" --json '["ana", "bob", "12.50"]'
# idempotent-replayed: true
# {"ok":{"id":1}}
```

- The client makes up the key (a UUID; 1 to 255 visible characters, else 400), and sends
  the same one when it sends the request again. A request with the key and another
  method, path, query or body is refused with 422. One that comes while the first is
  still running gets 409, and should be sent again a moment later. Neither runs.
- The answer kept is the status, the headers (not `Set-Cookie`) and the body; a request
  again gets it with `Idempotent-Replayed: true`. A streamed answer (`stream`, `events`)
  cannot be kept. Answers are kept 24 hours, then the key is new again: `{ max_age:
  "7d" }` (seconds, or a duration as for cookies, up to 366 days) keeps them longer.
  Expired ones are deleted as requests come. When the database that keeps them cannot
  be reached, the request is answered 503.
- Keys belong to the tenant, the mount point and the user: `session().user`, or the
  `user` option for a request that names its caller otherwise (`{ user:
  token.owner }`). The same key from another user is another request, and cannot read
  this one's answer. Without a user (no session), keys are the clients' own random
  values, shared by everyone who knows one.
- Without the header, `f` simply runs; `{ required: true }` answers 400 instead.
  Outside a request (a script, a job), `f` runs.
- **A plain form** cannot send a header, so `<form method="post" idempotent>` carries a key
  of its own, made as the page is rendered, in a hidden field (`_idempotency_key`) that
  `idempotent` reads where there is no header, `{ required: true }` included. The browser
  shows the answer to the last of two clicks, so that one waits for the first and gets
  its answer, not a 409, and the form edited and sent again is another request, not a
  422. A page with such a key is never kept by a cache.

  The whole round trip: the page with its form, and the route it posts to, which places
  the order once however many times the form is sent (a double click, the browser sending
  it again after an answer was lost):

  ```quillo
  db.open("shop.db")
  db.exec("create table if not exists orders (id integer primary key, item text not null, qty int not null)")

  route GET /order =
      <html><body>
          <form method="post" action="/order" idempotent>
              <input name="item" value="Coffee beans"/>
              <input type="number" name="qty" value="1"/>
              <button>Place the order</button>
          </form>
      </body></html>

  route POST /order { form: { item: str, qty: int } } = idempotent(() => {
      let o = db.one("insert into orders (item, qty) values (?, ?) returning id", item, qty)
      respond(201, <p>Order {o.id} placed: {qty} × {item}</p>)
  })
  ```

  The page the browser gets has the key in the form, a new one each time the page is made:

  ```html
  <form method="post" action="/order"><input type="hidden" name="_idempotency_key" value="5760b74f-41cb-4067-a450-986f06988301">...
  ```

  The form sent twice with that key is one order. The second answer is the first one,
  with `idempotent-replayed: true`, and the table has one row:

  ```sh
  curl -i localhost:8080/order -H "Origin: http://localhost:8080" -d item=Tea -d qty=2 -d _idempotency_key=5760b74f-...
  # HTTP/1.1 201 Created
  # <p>Order 1 placed: 2 × Tea</p>
  curl -i localhost:8080/order -H "Origin: http://localhost:8080" -d item=Tea -d qty=2 -d _idempotency_key=5760b74f-...
  # HTTP/1.1 201 Created
  # idempotent-replayed: true
  # <p>Order 1 placed: 2 × Tea</p>
  ```

  The visitor who clicked twice sees "Order 1 placed" once, as the page the second click
  brings. Loading `/order` again gives a new key, so a second order on purpose is a new
  order (2), and so is the form sent again with `qty` changed.
- In a function a service answers with, the key is the call's own id, which is the same
  for each try, so a step is one word: `server fn charge(ref: str, cents: int) =
  idempotent(() => ...)`. Its value is kept and given again, and what would refuse a
  request is a runtime error there, so the call is sent again. It holds across a crash,
  which a service's memory of its calls does not (see [Services](#services)).
- `idempotent` answers the request with what `f` gives, as the route or server fn would
  have: it ends the request, so make it the last thing. What comes before it (an
  `auth.required`, a pattern, a 400 for a bad form) runs every time and is not kept,
  as Stripe keeps nothing for a request that failed validation.
- A server fn whose body calls it gets a key from the page with each call. The page sends the
  call again, with its key, when the connection fails, or the answer is 409 or 502 to
  504: up to three more times, a quarter, half and a whole second later.

**What is guaranteed.** The answers are kept in `_quillo_idempotency`, in the program's
database (the tenant's, with a database per tenant), and `f` runs in a transaction on
that database, which `idempotent` begins (so it takes `isolation` and `retries`, and is
called outside `db.transaction`). The answer is written in that transaction, before it
commits: the work and its answer are kept together, or neither is.

- Before running, the key is claimed in a statement of its own, so that another request
  with it sees it running (409).
- When `f` returns, its answer is written and the transaction commits; then the answer
  is sent. When `f` fails, returns an error value or replies early, the transaction
  rolls back, the claim is given up, and the answer is sent but not kept: nothing
  happened, so a retry runs again. To keep a refusal ("not enough money"), return it:
  `respond(422, { error })` commits (what `f` wrote with it too) and is kept. A server
  fn's error value is not kept: the page's retry asks again.
- If the server stops (or loses the database) part way, the database rolls the
  transaction back, and the claim is left. A minute after it was made, the next request
  with the key takes it over and runs. If the first was only slow, and is still running
  then, at most one of the two commits: the claim is taken over only while it is not
  answered, and an answer is written only by the claim's holder, in its transaction; the
  other rolls back and gets the first's answer, or 409.
- On SQLite a transaction holds the database, so a second request with the key that
  comes while the first runs waits for it, and gets its answer. On PostgreSQL and MySQL
  it gets 409.
- Work outside that database is not in the transaction: `http.post` to a payment
  provider, another `db.connect`, files. For a handler that is mostly that,
  `{ transaction: false }` runs `f` without one, and keeps every answer, failures too
  (as Stripe does), since part of the work may have happened. A server that stops there
  leaves the key running: it is not known whether the work happened, so requests with
  the key get 409 until it expires. Give the provider the same key, if it takes one,
  so that it too makes the payment once.

## Prefetching {#routes-prefetching}

Every page a route makes asks for the pages it links to before they are clicked: a
link of the same site pointed at (for 65 ms), pressed, touched or reached with Tab. By
the time the click lands, the page is there. Browsers with speculation rules (Chrome,
Edge) keep the page for the navigation themselves; others get it in the background and
keep it in their cache for 30 seconds, which the server allows for this, unless someone
has logged in or out.

- **Prefetching changes nothing.** A route asked for ahead of a click may read (the
  database, files, other servers) but not write: `db.exec` or SQL that writes, assigning
  a global variable, cookies and sessions, files, `publish`, `http.post` and the like,
  and calls to services stop it, and it is answered "not prefetched" (503, which the
  browser drops; the request log shows it as any 503, with no error line). The page is
  then made, as usual, when the link is followed. So a `GET` that does something (a
  logout link, a counter) is never done by pointing at it. A page prerendered runs none
  of its components, `server fn`s or live updates until someone opens it.
- **Links a page's components follow** are fetched for the state of the components on the
  page they lead to, so those start with their data and no `null` first (see
  [Single-page apps](#single-page-apps)). It is kept for 30 seconds, until a `server fn` is
  called, and only for components not already on the page; the others ask as usual.
- **Keeping up with changes:** a request that changes something, or logs someone in or
  out, answers with `Clear-Site-Data: "prefetchCache", "prerenderCache"`, so a page kept
  for a click is not shown out of date; Chrome obeys it from version 138. A `GET` does not
  send it, nor a `POST` that only reads. Elsewhere, a change that is not a login can be up
  to 30 seconds old on the click.
- **When the server is busy:** with anyone waiting, a prefetch is refused before it runs,
  so prefetches never go ahead of a visitor.
- **Seeing what it does:** the log marks them, `GET /docs -> 200 (0.4ms) prefetch`, and
  `"prefetch": true` under [`--log json`](#options-of-quillo-run). Started with [`--metrics`](#options-of-quillo-run)
  (`quillo run main.qwe --log json --metrics`), `quillo_prefetches_total` counts
  those `answered`, `refused`, and `used` by a click, as a page reports in Chrome and Edge.
- **What is left alone:** links to other sites, with `target` or `download`, to the
  same page (`#section`), anything inside an element with `data-no-prefetch`, and every
  link on a connection saving data.
- A route that sets its own `cache-control` keeps it. Starting the program with
  [`--no-prefetch`](#options-of-quillo-run), `quillo run main.qwe --no-prefetch`, turns prefetching off.
- **Pages sent as written:** a file `file()` or `files()` serves, and any answer with an
  `etag` or a range, goes out as its bytes are, so its `ETag` and `Content-Range` name what
  is sent: no prefetching script is added to it, nor [`--watch`](#options-of-quillo-run)'s.

## Streaming

A route can send its answer as it makes it: a large export row by row, a job's
progress, a model's reply word by word. The client has the head at once, and each
`send` as it is made.

```quillo
route GET /orders.csv = stream((send) => {
    send("id,total\n")
    for o in db.query("select id, total from orders") { send(`${o.id},${o.total}\n`) }
}, { type: "text/csv", headers: { "content-disposition": "attachment" } })

route GET /jobs/:id/progress = events((send) => {
    for step in range(10) {
        work(id, step)
        send({ done: step + 1, of: 10 }, "progress")   // a value, and an event name
    }
})

// A model's answer, passed on as it arrives.
route POST /ask = events((send) => http.stream("https://api.example.com/v1/chat", send, {
    body: { prompt: request.json.prompt, stream: true },
    headers: { authorization: `Bearer ${env("API_KEY")}` },
}))
```

- `stream(f)` sends what `send` is given: text as it is, other values as a line of
  JSON (newline-delimited JSON, with `type: "application/x-ndjson"`). `events(f)`
  sends Server-Sent Events: each value as JSON in an event's data, with a name if
  `send` is given one; `send(null)` is a comment, which keeps a quiet connection
  open. `send(value, { event, id, retry })` names the event, gives it an id, which the
  browser sends back as `Last-Event-ID` when it connects again, and says how many
  milliseconds a client waits before it does; `send(null, { id: "8" })` sends an id
  alone. Another option, or a `retry` that is not a whole number, ends the stream with
  an error. A program that goes on from the last id reads it in
  `request.headers["last-event-id"]`:

  ```quillo
  route GET /feed = events((send) => {
      let from = int(request.headers["last-event-id"] ?? "0") ?? 0
      for n in range(from + 1, from + 100) {
          send({ n }, { event: "tick", id: str(n), retry: 2000 })
          sleep(1000)
      }
  })
  ```
- An HTTP/1.0 client, which reads no chunks, gets a stream with no length, ended by
  closing the connection.
- The rest of the server carries on meanwhile. A `send` waits only when the client
  reads slower than the program writes, and then it waits as a database call does,
  holding up no other request. Proxies are asked not to hold the answer back.
- When the client goes away (or the server stops), the next `send` ends the
  function, so a stream that runs until someone leaves stops when they do. An
  error in the function after the answer began cuts the connection, so the client
  knows the answer is incomplete.
- `http.stream(url, f, { method, body, headers })` reads another server's answer as
  it arrives: `f` gets each event (its data, parsed as JSON when it is, its name, its
  id and its `retry`: `(data, name, id, retry)`, the id being the last one the stream
  gave, as a browser keeps it), each line of newline-delimited JSON, or each piece of
  text, and may stop it by returning `false`. Given `send`, what arrives goes straight
  on. It gives `{ status, ok, headers, cookies }` once the answer has ended (with the
  `body` when it is not a success), or an error value when it could not be had (also
  when one event or line goes past 16 MiB without ending). The map also takes
  `redirects`, `public_only`, `ca`, `cert` and `key` (see [HTTP client](#http-client)).
- In a page, `receive(url, f, { method, body, headers })` in a handler sends the
  request (a file from an input as `body` goes as it is) and calls `f` with what the
  answer streams, until it ends; `listen(url,
  f)` in a component's code follows a stream for as long as the code that called it
  stands, and opens it again if the connection drops, sending the last event's id as
  `Last-Event-ID` and waiting the `retry` the server gave, if it gave one:

```quillo
component Chat() {
    state answer = ""
    <div>
        <button @click={() => receive("/ask", (part) => { answer = answer + part.text }, { body: { prompt: "hi" } })}>Ask</button>
        <p>{answer}</p>
    </div>
}

component Progress(job: int) {
    state done = 0
    listen(`/jobs/${job}/progress`, (p) => { done = p.done })
    <progress value={done} max="10" />
}
```

Each open `listen` holds one of the six connections a browser keeps to a site over
HTTP/1.1, across its tabs. That suits a job's progress or a reply being written; for
data every page follows, `live` (see [Live data](#live-data)) shares one WebSocket.
Outside the server (a program's own requests in tests), the function runs first and
the whole answer is sent at once.

## UDP

A program can take UDP packets on a port, answer them, and send its own: readings from
sensors, syslog, a game's moves, metrics.

```quillo
let readings = []

udp.listen(9000, (p) => {
    readings = readings + [json.parse(p.text)]   // pages with live(...) on it follow
    udp.reply(p, "ok")                           // sent back from port 9000
})

udp.send("10.0.0.5", 514, "<14>app started")
```

A page that shows the last readings as they come, and a sensor's packet sent from a
terminal (`nc` stands in for the device):

```quillo
server fn latest() = readings | drop(len(readings) - 10)     // the last ten

component Readings() {
    state rs = live(latest)
    <ul>{for r in rs => <li>{r.sensor}: {r.c} °C</li>}</ul>
}

route GET / = <html><body><Readings/></body></html>
```

```sh
echo '{"sensor": "hall", "c": 21.5}' | nc -u -w1 127.0.0.1 9000     # prints ok, the reply
```

Each packet adds a line, `hall: 21.5 °C`, to every page open on `/`, without a reload.

- `udp.listen(port, f, { host, queue })` is written at the top of a program. The port is
  taken when the program runs, so one that is in use is an error at once. `host` is the
  address to listen on: every IPv4 address, `"0.0.0.0"`, unless given; `"::"` for IPv6
  as well. There is no multicast, and packets are plain: no DTLS.
- `f` gets each packet: `{ text, bytes, from: { host, port }, port, at }`. `text` is the
  packet as text, or `null` when it is not UTF-8; `bytes` is its bytes as a list of
  numbers; `at` is when it came, in milliseconds since 1970. A handler written with no
  parameter is called with none.
- The handler runs like a request: waiting on a database or `http.*` holds up no one, and
  it sees the program's globals, so a page following them with `live` sees each packet.
  Packets reach it in the order they came, one at a time. An error in it is logged and
  the next packet goes on.
- Packets wait in a queue while the handler is busy (10,000 unless `queue` says
  otherwise). A full queue drops its oldest packet, as UDP would; on a server started with
  [`--metrics`](#options-of-quillo-run) (`quillo run main.qwe --metrics`),
  `quillo_udp_received_total` and `quillo_udp_dropped_total` count them. Nothing slows
  a sender down, so where packets come in bursts faster than the handler takes them,
  give `queue` room for the largest burst.
- `udp.send(host, port, data, { from })` sends one packet; `udp.reply(packet, data)` sends
  back to where a packet came from, from the port it came to (as most protocols expect).
  `data` is text, a list of byte values, or any other value as JSON; a packet holds up to
  65,507 bytes. Both return `null`, or an error value when the host has no address or the
  system refuses the packet; more than 65,507 bytes, `null` data, or a `from` port the
  program does not listen on is a runtime error. A host name with both kinds of address
  is sent to over IPv4. They need the `net` effect.
- With [`--reload`](#options-of-quillo-run) and [`--watch`](#options-of-quillo-run), a port the program still listens on stays open, and its
  packets go to the new version's handler; a port it stopped listening on is closed
  (after 3 s, in case the next version takes it back).
- One program on a machine listens on a port: run with [`--instances`](#options-of-quillo-run), the instances share
  the socket. A program mounted in another (`mount /x = "x.qwe"`) gets its own ports' packets.

## Channels

A channel carries messages between the server and its pages as they happen: moves in a
game, cursors, readings from a device, a chat. Pages send to the program, which answers
one page, all of them, or none.

```quillo
channel.on("game", (msg, peer) => {
    // Everyone else in the channel sees the move.
    channel.broadcast("game", { id: peer.id, pos: msg.pos }, { except: peer })
})

component Board() {
    state others = {}
    let game = channel.open("game", (m) => { others = set(others, m.id, m.pos) })
    <canvas @pointermove={(e) => game.send({ pos: [e.clientX, e.clientY] })}/>
}
```

- In a component, `channel.open(name, f)` opens a channel for as long as the code that
  called it stands (the component leaving the page closes it): `f` gets each message the
  server sends on it, and `.send(value)` sends one to the server. On the server, while
  the page is rendered, it opens nothing.
- On the server, `channel.on(name, f, { join, leave })` is written at the top of a
  program, once for a name. A name is letters, digits, `_`, `-`, `.` and `:`, at most
  64 characters. `f(msg, peer)` gets each message from a page; `peer` is `{ id,
  session }`, its `id` twelve hex characters made for the connection (a page that
  reconnects has a new one), and `session()` in `f` is the page's session too, as it
  was when the page connected: someone who signs in afterwards is seen so once the page
  reconnects. So is its tenant. Messages reach `f` in the order they came, one at a time,
  as UDP packets do (see [UDP](#udp)): like a request, with waits holding up no one. An
  error in `f` is logged and the next message goes on. Each of `f`, `join` and `leave`
  is called with as many arguments as it declares.
- `join(peer)` decides whether a page may join (anything but `null`, `false` or an error
  says yes): anyone who can open a page can ask, so a channel whose messages are not for
  everyone has one (`(peer) => peer.session.user != null`). `leave(peer)` hears that a
  page left, or closed (for each channel it was in; not when the server itself
  stops). A channel no `channel.on` names may be joined, to hear what the program
  broadcasts on it; what pages send on it goes nowhere. There is no list of who is
  present beyond `channel.peers` and what `join` and `leave` record. What `join`
  broadcasts on its channel, or sends to the page that is joining, reaches that page
  too, right after its yes: a roster broadcast as someone enters is the first thing
  they hear. So does what is broadcast between the yes and the page hearing it.

  A chat for people who are signed in, with both sides:

  ```quillo
  channel.on("lobby", (msg, peer) => {
      channel.broadcast("lobby", { from: peer.session.user, text: msg.text })
  }, {
      join: (peer) => {
          if peer.session.user == null { return false }      // signed-in people only
          channel.broadcast("lobby", { from: peer.session.user, text: "joined" })
          true
      },
      leave: (peer) => channel.broadcast("lobby", { from: peer.session.user, text: "left" }),
  })

  component Lobby() {
      state lines = []
      state draft = ""
      let room = channel.open("lobby", (m) => { lines = lines + [`${m.from}: ${m.text}`] })
      <div>
          <ul>{for l in lines => <li>{l}</li>}</ul>
          <form @submit={() => { room.send({ text: draft }); draft = "" }}>
              <input value={draft}/>
              <button>Send</button>
          </form>
      </div>
  }

  route GET / = <html><body><Lobby/></body></html>
  ```

  Ana, signed in, sees `ana: joined` first, then each line anyone sends, her own too
  (`ana: hi all`), since the broadcast has no `except`. A visitor with no session is
  refused at `join`: the list stays empty, and what they send goes nowhere.
- `channel.broadcast(name, value, { except, reliable })` sends to every page in the
  channel and gives how many took it; `channel.send(name, peer, value, { reliable })`
  sends to one (the peer, or its id), and gives whether it was there;
  `channel.peers(name)` gives the ids of the pages in it. They need the `net` effect,
  and wait for nothing: a page that cannot keep up misses messages rather than holding
  up the program. A message is a value, as JSON, up to 1 MiB (bytes go as a list of
  numbers; there is no binary message). Each page has room for 4096 messages on their
  way to it: when it is full, a plain message is dropped and counted, and a `reliable`
  one closes the connection, so the page comes back and joins again rather than miss
  it. A channel keeps 10,000 messages from pages waiting for its handler; past that the
  oldest goes. Nothing is kept for a page while it is away.
- In the page, messages sent before the join is answered, or while the connection is
  down, wait (up to 256 a channel) and go once it is joined; sent on a channel that
  refused the page, they are dropped. `.send` gives `null`.

How messages travel:

- A page opens one connection for all its channels. Where the browser has WebTransport
  (HTTP/3, over UDP), a message that fits in a datagram (about 1,100 bytes as JSON) goes
  as one: it arrives as soon as it can, may come out of order, and may be lost, as a
  game's moves or a sensor's readings can afford to be. Larger messages, and those sent
  with `{ reliable: true }` (from either side: `game.send(v, { reliable: true })`), go on
  a stream: all of them, in order.
- Where WebTransport cannot be had (the browser lacks it, UDP is blocked, the page is
  served from a name its certificate does not cover, or it is not ready within 1.5 s),
  the page uses a WebSocket for the rest of its life, which carries every message
  reliably and in order (`reliable` changes nothing there). The connection is opened
  again when it drops (after half a second, then longer, up to 15 s), the channels
  joined again, and what was queued sent. A page that also shows live data has two
  connections, `/_quillo/live` and `/_quillo/wt`. In a phone app or `quillo app`, which
  have no socket, both go through the app's bridge instead.
- The server listens for WebTransport on the UDP port with the HTTP port's number
  (start the program with [`--wt-port`](#options-of-quillo-run) for another, `quillo run main.qwe --wt-port 4433`), from when the first page asks to connect, on every address
  of the machine unless [`--host`](#options-of-quillo-run) is a loopback address. It makes a certificate of its
  own, good for 13 days and replaced after 11, which browsers take by its hash (they
  take such certificates for two weeks at most); [`--tls-cert`](#options-of-quillo-run) and [`--tls-key`](#options-of-quillo-run), given
  together, give it a real one. A WebTransport connection is kept alive every 5 s and
  dropped after 20 s of silence. Behind a proxy that passes HTTP only, open the UDP
  port to the server itself, or pages use the WebSocket. [`--no-wt`](#options-of-quillo-run) turns WebTransport
  off, as does a build without the `realtime` feature.
- Broadcasts reach the pages connected to this server (its [`--instances`](#options-of-quillo-run) share them;
  servers behind limer do not relay them, see [Several servers](#several-servers)).
  With [`--metrics`](#options-of-quillo-run), `quillo_channel_pages` counts them and `quillo_channel_dropped_total`
  the messages dropped because a queue was full. The server keeps channels in memory:
  after a restart, pages reconnect and join again, with new ids, and queued messages are
  what each page kept.
- There is no WebSocket or WebRTC of your own: pages talk to the server, never to each
  other directly, and a channel is the way a page's messages reach the program.

UDP and channels together pass a device's packets on to pages as they come; see
[telemetry.qwe](https://github.com/quillolang/examples/blob/main/telemetry.qwe).

## Media

A program can take a camera's or an encoder's stream as RTP, or a page's camera, and
play it in its pages, a fraction of a second behind:

```quillo
media.rtp("cam", { video: 5004, audio: 5006 })

route GET / = <html><body><quillo-media stream="cam" controls muted/></body></html>
```

```sh
ffmpeg -f avfoundation -framerate 30 -video_size 1280x720 -i "0:0" \
  -map 0:v -c:v libx264 -tune zerolatency -profile:v baseline -pix_fmt yuv420p -g 60 \
  -f rtp rtp://127.0.0.1:5004 -map 0:a -c:a libopus -f rtp rtp://127.0.0.1:5006
```

- `media.rtp(name, { video, audio, host })` makes a stream: at the top of a program, or
  while it runs (a route, a channel's handler: a stream for each room), once for a name
  (asked again, it is the same stream; names are as channels'). Video is
  H.264 and audio Opus, the only codecs taken, each on a port of its own (`{ port: 5004,
  codec: "h264" }` says so in full). The packets never reach Quillo code: the server puts
  them back in order (waiting 50 ms, or 256 packets, for a late one), makes frames of
  them (it reads single NAL units, STAP-A and FU-A), and passes each on to every page
  watching, without decoding or encoding anything. Any sender that reaches the port is
  taken (the payload type and SSRC are not checked), and there is no RTCP and no SRTP:
  keep the ports to the network the camera is on. The server keeps at most 600 frames
  since the last keyframe.
- `<quillo-media stream="cam">` in any page plays the stream: the page loads a small
  script for it (4.5 KB compressed, `/_quillo/media.js`, put into any page that writes
  the element), which decodes with the browser's own decoders (WebCodecs) and draws on a
  canvas, about 120 ms behind the frames it has. `controls` adds a button for sound, and
  `muted` starts without it (browsers play sound only after a click on the page; a muted
  player does not decode the sound at all). Its `data-state` is `connecting`,
  `waiting`, `playing`, `ended` or `unsupported` (no WebCodecs), for styling, and
  `el.mediaStats` counts what it decoded.
- Frames travel on the page's channel connection: over WebTransport, a stream per
  track; else over the WebSocket. The server keeps the video since the last keyframe, so
  a page that starts watching shows a picture at once. A page that falls behind skips
  to the next keyframe, and so does a stream that lost a packet of video.
- A connection that drops is opened again (after half a second, then longer, up to 15
  seconds) and the page goes on watching, as a page that starts does. With no frames for 3
  seconds (the camera stopped, the connection is down), `data-state` is `waiting`.
- `media.streams()` gives a list, by name: `{ name, viewers, video: { frames, fps,
  kbps, lost }, audio: { ... } }`, the viewers being this server's pages, and `fps` and
  `kbps` 0 after 3 s without frames.
- **Pages send too.** `media.on(name, { publish })` lets pages send stream `name` (or
  every stream a name ending in `*` starts: `"call:*"`), and
  `<quillo-media publish="call:42">` sends the page's camera and microphone to it, for
  every `<quillo-media stream="call:42">` to play. The browser encodes them with WebCodecs
  (H.264 baseline at 1280x720, 30 frames a second and 1.5 Mbit/s with a keyframe every
  two seconds, and Opus at 64 kbit/s, what the relay passes on) and sends the frames on
  its channel connection; the server passes them on as it does RTP, decoding nothing.
  Sound needs Chromium's capture API (`MediaStreamTrackProcessor`): a browser without it
  sends the picture alone. `publish(peer)` says which pages may, as a channel's `join`
  does (with the page's `session()`); without it, any page may. A video call is a
  stream per person:

  ```quillo
  media.on("call:*", { publish: (peer) => session().user != null })

  component Call(room: str, me: str, others: list) {
      <div>
          <quillo-media publish={`call:${room}:${me}`}/>
          {for p in others => <quillo-media key={p} stream={`call:${room}:${p}`}/>}
      </div>
  }
  ```

  `no-video` or `no-audio` leaves one out. Given a MediaStream as its `source` (a
  canvas's, the screen's: `el.source = js("navigator.mediaDevices.getDisplayMedia")` in
  `@mount`), it sends that instead of the camera. Its `data-state` is `starting`,
  `sending`, `refused` (no camera, or `publish` said no), `ended` or `unsupported`, and
  `el.mediaStats` counts what it encoded and sent. Each person's picture goes once to the
  server, which sends it on to everyone watching (what video services call an SFU); there
  is no TURN relay for networks that block UDP, where the WebSocket carries it.
- `media.close(name)` ends a stream: it stops taking RTP, lets its ports go (for another
  stream to take; it waits up to a second for that), and tells its pages, whose
  `data-state` turns `ended`, and stays so until the page reconnects, even if a stream
  of that name is made again. It gives true if there was such a stream.
- Streams are not recorded: frames go to the pages watching, and nowhere else. Write
  a recording with the encoder (ffmpeg's `-f tee`) beside what it sends. A stream lives
  on the server its packets come to: pages on another server behind a balancer do not
  see it.
- Audio and video are each played as they come, so they are in step within about a
  tenth of a second. The camera's encoder decides the picture: keyframes every second
  or two (`-g 60` at 30 fps) let pages that start watching (or fall behind) catch up
  quickly; `-tune zerolatency` keeps the encoder from holding frames back.

## Static files

```quillo
route GET /assets/*path = file("public/" + path)
route GET /build/*path = file("dist/" + path, { cache: "1y" })   // names that change with content
```

- `file(path, options)` takes three options and refuses any other, naming those it takes:
  `cache` (below), `download`, a name, which sends `Content-Disposition: attachment` with
  that name (written so a name in any script arrives whole), and `type`, the content type
  in place of the one the extension gives: `file("exports/r.csv", { download: "rapport
  d'été.csv", type: "text/csv; charset=utf-8" })`.
- Each file goes out with an `ETag` (a weak one, from the file's size, time, inode and
  change time) and `Last-Modified`, and as it is on disk: nothing is added to an HTML
  file, so a range of it is the bytes its `Content-Range` names. A browser asking with a
  current copy (`If-None-Match`, or an `If-Modified-Since` the file has not changed
  since, in any of HTTP's three date forms) gets a `304` with no body. `If-Match` that
  does not name the file's `ETag`, or an `If-Unmodified-Since` it has changed since, gets
  a `412`, on a `GET` as on a write. A file replaced by one of the same size and
  modified time (`cp -p`, `touch -t`, a build that sets every file's time) is a new
  file, in memory and to the browser.
- The type comes from the extension: `html htm css js mjs json map webmanifest xml xsl
  txt csv svg png jpg jpeg gif webp avif ico mp4 m4v webm mp3 ogg oga wav wasm woff2
  woff ttf otf pdf zip`, and `md` (`text/markdown`), `vtt` (subtitles for `<track>`),
  `ics`, `rss`, `atom`, `jsonld`, `geojson`, `yaml`, `yml`, `toml`, `m3u8`, `mpd`, `ts`,
  `m4s` (HLS and DASH), `srt`, `tsv`, `bmp`, `tif`, `tiff`, `heic`, `jxl`, `mov`, `m4a`,
  `aac`, `flac` and `opus`, text ones with `charset=utf-8`; anything else is
  `application/octet-stream`, which `nosniff` keeps a browser from guessing at (give
  `type` then). `file()` is also the only
  way to send bytes as they are: a route's other answers are text, HTML or JSON, so a
  file made in the program (a PDF, an image from `image.convert`) is written under the
  program's folder and served from there.
- `Cache-Control` is `no-cache` (keep it, but ask each time) unless `cache` is given:
  seconds, or `"10m"`, `"12h"`, `"30d"`, `"1y"`, which allows keeping it that long
  without asking (`immutable`). Use it for files whose name changes with their content.
- `Range: bytes=…` gets a `206` with only those bytes (audio and video seeking; `If-Range`
  is honoured); a range past the end gets a `416`, and several ranges at once the whole
  file.
- Files up to 1 MiB are kept in memory (64 MiB in all) and re-read when they change on
  disk, or are replaced. Larger ones are sent from disk as they are written, on a thread of their own
  with a 64 KiB buffer, so a video costs no more memory than a page. At most 256 such
  sends run at once, 8 for one client (an IPv4 address, or an IPv6 /64 network; past
  that, a `503` with `Retry-After`). A reader slower than 16 KiB/s is let go after 20
  seconds, and any send stops after an hour: a browser or player goes on from there
  with a `Range` request. Behind a proxy every visitor shares the proxy's address and so
  its 8 places; limit clients at the proxy instead. The gzipped copy of a text file up
  to 1 MiB is kept too (32 MiB of them in all), rather than compressed again for each
  request.
- Paths with `..`, or starting at `/`, get a `404`.

A folder of files, as a static host serves it, is `files(folder, path, options)`:

```quillo
route GET /*path = files("site", path)
route GET /docs/*p = files("manual", p, { index: "home.htm", extensions: ["htm"], not_found: null, cache: "1h" })
```

- `/` and `/docs/` are their folder's `index.html`, `/about` finds `about.html`, and a
  folder named without its `/` (`/docs`) is redirected to it (301), so its pages'
  relative links work. A miss is the folder's `404.html`, sent with status 404, when it
  has one; else the plain 404.
- Files whose names start with a dot (`.env`, `.git/`) are not served, but `.well-known/`
  is; nor are the program's own (`.qwe`, `.qwer`, `.rs`). Each file goes as `file()`
  sends it, its `ETag`, ranges and type included. A path that leads out of the program's
  folder through a link is a miss, whether what it leads to is there or not (see [Paths
  and links](#paths-and-links)). `files` and `file` need the `fs` effect.
- The options: `index` (a file name, `index.html` unless given), `extensions` (those a
  clean URL may leave out, `["html"]` unless given), `not_found` (the page for a miss,
  `404.html` unless given; `null` for none) and `cache`, as `file()`'s. Another option is
  a runtime error that lists them.

`quillo serve site` serves a folder that way with no program at all: `index.html`, clean
URLs and `404.html`, on [`--port`](#options-of-quillo-run) (8080 unless given: `quillo serve site --port 3000`) and with
the other [options of `run`](#options-of-quillo-run).
A Markdown file goes out as `text/markdown`; to show one as a page,
`markdown_page(read_file("notes.md"))` gives a whole page (a doctype, a `<head>` with
`<title>`, the first `#` heading unless `title` is given, and `lang` and `html` as
options; `html: true` keeps HTML written in the text, as `markdown` does).

A file the program makes as it starts, a stylesheet compiled by an engine in Rust, is
declared with `build` at the top level:

```quillo
use "tailwind.rs"

build "/app.css" = tailwind(css`@import "tailwindcss";`, classes())
```

- It is made once the port is open, beside the requests, where work at the top level
  (`let APP = tailwind(...)`) holds the port shut until it ends. A request for it that
  comes first waits for it, and is not refused. The log says when it is done: `quillo:
  built /app.css in 1.5 s`.
- It is served at its path with the type its extension names (text; markup as HTML;
  another value as JSON; a `respond(...)` as it is), an `ETag` and `cache-control:
  no-cache`, so a browser's copy is checked rather than fetched again.
- One that fails at the first start stops the program with exit 1 (`quillo: build /app.css
  failed: no engine`). After a reload ([`--watch`](#options-of-quillo-run), [`--reload`](#options-of-quillo-run)) it is made again, and while
  that fails the last good one is served.
- The path starts with `/`, and holds no `?` and no trailing `/` (`` `build` names the path
  it is served at ``). Elsewhere `build` is an ordinary name.
- Ordinary top-level code still runs before the port opens, as a route may need what it
  sets: only what is declared as a `build` moves off that path. `quillo build` does not
  make it ahead of time.

## Translations

```quillo
translations("locales")          // locales/en.json, locales/fr.json, locales/pt-BR.json

view Header(user) = <header>
    <a href="/">{t("nav.home")}</a>
    <span>{t("greeting", { name: user.name })}</span>
    <span>{t("cart.items", { count: len(user.cart) })}</span>
</header>
```

```json
{ "nav": { "home": "Accueil" },
  "greeting": "Bonjour, {name} !",
  "cart": { "items": { "one": "{count} article", "other": "{count} articles" } } }
```

- The folder holds one JSON object per locale, `<dir>/<locale>.json`, the locale named
  as `en`, `pt-BR` or `zh-Hant`. A file named with `_` (`fr_FR.json`, as other tools
  write them) is read as `fr-FR`; two files for one locale are an error.
- A request's locale is what `locale_from((request) => ...)` gives (a query parameter,
  a user's setting), else its `locale` cookie, else the best match in `Accept-Language`
  (`fr-CA` is served by `fr`, `pt` by `pt-BR`; `q` weights count, `*` does not), else
  the default: `translations(dir, { default: "en" })`, or `en` when there is one, or
  else the first locale by name. `locale()` gives it. A choice of language a visitor makes
  is that cookie, set by a route the page's form posts to:

  ```quillo
  translations({ en: { hi: "Hello", lang: "Language" }, fr: { hi: "Bonjour", lang: "Langue" } })

  route GET / = <html lang={locale()}><body>
      <h1>{t("hi")}</h1>
      <form method="post" action="/lang">
          <select name="to">
              <option value="en" selected={locale() == "en"}>English</option>
              <option value="fr" selected={locale() == "fr"}>Français</option>
          </select>
          <button>{t("lang")}</button>
      </form>
  </body></html>

  route POST /lang { form: { to: str } } {
      if !contains(["en", "fr"], to) { reply 400 "No such language" }
      set_cookie("locale", to, { max_age: "1y" })     // read before Accept-Language
      redirect("/")
  }
  ```

  A browser that asks for `fr-CA` sees `Bonjour` at once; one that asks for nothing sees
  `Hello` until Français is chosen, and `Bonjour` from then on, with `<html lang="fr">`.
- The best match goes by script before language, with CLDR's likely scripts: `zh-TW`,
  `zh-HK`, `zh-MO` and `zh-Hant-*` get `zh-Hant`, and `zh-CN`, `zh-SG` and plain `zh`
  get `zh-Hans`; `sr-Latn-RS` gets `sr-Latn`, and `sr` gets `sr-Cyrl`. A locale in
  another script than the one asked is not taken over one that matches. A region takes
  CLDR's parent where there is one: `pt-AO` gets `pt-PT`, a Latin American `es-MX` gets
  `es-419`, and English outside the US gets `en-GB`.
- Nested keys are written with dots. A key missing in a locale comes from the default
  locale; a key missing there shows as itself.
- `{name}` is replaced by that value. An entry of plural forms (`zero one two few many
  other`) picks the form for `count` by the language's rules, CLDR's, which are built in,
  the same table on the server and in the browser: English and the languages that count
  as it, French, Brazilian Portuguese and Hindi (where 0 is singular), European
  Portuguese (`pt-PT`, where only 1 is), Russian, Ukrainian and Belarusian, Serbian,
  Croatian and Bosnian, Polish, Czech and Slovak, Romanian (0, 2 to 19 and x01 to x19 are
  `few`), Lithuanian, Latvian, Slovenian, Icelandic, Macedonian, Irish, Welsh, Maltese,
  Arabic, Hebrew, and `other` alone for Japanese, Chinese, Korean, Thai, Vietnamese,
  Indonesian, Malay and their kin. French, Spanish, Italian, Catalan and Portuguese take
  `many` for exact millions (`1000000`). A `zero` form, where one is written, is used for
  0 in every language.
- `count` may be a number, or numeric text (`"1"`, as a query or a form gives it), taken
  at its number. A fraction takes the language's form for fractions: `many` in
  Lithuanian, Czech and Slovak, `few` in Romanian, `other` elsewhere. A decimal or text
  written with a fraction's digits counts as a fraction, as CLDR has it: `1.0m` and
  `"1.0"` take `other` in English ("1.0 items").
- As it loads, a form the language never picks is warned of: ``translations (en): `a` has
  a `few` form, which en never picks (it has one, other)``.
- Translations are text, escaped in pages like any other; `raw(t(...))` for markup you
  wrote in them. In that form the values put in (`{name}`) are still escaped, on the
  server and in the browser: `raw(t("welcome", { name: user.name }))` keeps the
  translation's `<b>` and shows the name as text. (Only that form: `raw` of a variable
  holding `t(...)`'s text makes all of it HTML.)
- `with_locale("de", () => ...)` runs code in another locale (an email in the
  recipient's language).
- `translations({ en: {...}, fr: {...} })` takes the text directly instead of a folder.
- Components call `t` and `locale` too. A page with components loads its locale's text
  from `/_quillo/t/<hash>.js`, which the browser keeps, rather than carrying it in each
  page. It holds only the keys the components' code can ask for: each `t("literal")`,
  every key that starts with a template's literal start (`` t(`sec.${x}`) `` sends
  `sec.*`), and the `quillo.*` keys. A key worked out any other way (`t(k)`) sends all
  of them. Cached views are kept once per locale.
- `ask()`'s buttons follow the page's locale: the program's own `quillo.ok` and
  `quillo.cancel` translations when it has them, else Quillo's own for about thirty
  languages (Spanish Aceptar and Cancelar, French, German, Japanese, Chinese, Arabic,
  ...), else English.

## Mounting programs

`mount` serves another `.qwe` program under a path prefix:

```quillo
mount /notes = "notes.qwe"
mount /shop = "shop/app.qwe"
route GET / = <a href="/notes">Notes</a>
```

The mounted program runs unchanged, with its own globals and its own database, and
sees paths without the prefix (`url().path` is `/products`, not `/shop/products`).
Its root-relative links and form actions (`href`, `src`, `action`), its redirects, its
browser bundle, its `server fn` calls, live data and page changes are all moved under the
prefix. Its cookies are set for the prefix (`Path=/shop`), its session cookie is
`quillo_session` and is signed for that mount, so the two programs' sessions never pass
for each other. Its file paths count from its own folder; the services it declares are
shared with the program that mounts it. `/_quillo/health` and `/_quillo/metrics` are
the process's, at the root only. `mount`, like `service`, is written at the top level.
Changes to mounted files are picked up by [`--watch`](#options-of-quillo-run) (`quillo run main.qwe --watch`). See
[index.qwe](https://github.com/quillolang/examples/blob/main/index.qwe), which mounts every example.

## Services

A part of an app can run in the same process or as a server of its own, without its
code changing. `service` names a program whose `server fn`s the others call:

```quillo
// billing.qwe: the service
db.open("billing.db")
db.exec("create table if not exists invoices (id integer primary key, name text not null, cents int not null)")

server fn invoice(name: str, cents: int) -> map =
    db.one("insert into invoices (name, cents) values (?, ?) returning id", name, cents)

// shop.qwe
service billing = "billing.qwe"

route GET / =
    <form method="post" action="/buy">            // /shop/buy once served under /shop
        <input name="name" placeholder="Your name" required/>
        <button>Buy, 12.00</button>
    </form>

route POST /buy {
    let inv = billing.invoice(request.form.name, 1200)?    // billing's `server fn invoice`
    <p>Invoice #{inv.id}</p>
}

// app.qwe: the app, made of both
service /shop = "shop.qwe"          // named after the file; its routes under /shop
service /billing = "billing.qwe"
```

`quillo run app.qwe` serves the form at `/shop/`; sent, it shows `Invoice #1`, the row
billing wrote in its own database. The shop's own paths are written as the shop sees
them: `action="/buy"` reaches the page as `/shop/buy`, as the shop's links and redirects
do (`action="/shop/buy"` would become `/shop/shop/buy`). `quillo run app.qwe --split`
answers the same, with billing in a process of its own.

- **In one process** (the default) a service is loaded once and shared by every program
  in the process that names it: one copy of its variables and its database. A call
  goes straight to it.
- **Split**, a service runs elsewhere: the app started with the option
  [`--service-url`](#options-of-quillo-run), `quillo run app.qwe --service-url billing=http://10.0.0.5:8081` (or
  `QUILLO_SERVICE_BILLING`) sends its calls there, `POST /_quillo/service/invoice`,
  signed with HMAC-SHA256 under `QUILLO_SERVICE_SECRET` (the same on every service), or
  proven another way (see [Authentication between
  services](#authentication-between-services)). The signature covers the time (a call
  more than five minutes old is refused), the service it is for, the function, the
  tenant, the arguments, the call's id, the caller's name, its deadline and its
  contract, and the answer comes back signed with the same key. A
  service takes only calls for itself: its name is the program's file name
  (`billing.qwe` is `billing`), or [`--service-name billing`](#options-of-quillo-run) when the file is named
  otherwise. It remembers each call's id for longer than a signature lasts, so a call
  sent again, by a retry or by someone who saw it on the network, is answered as it
  was and not run again. A service with a database keeps that memory there too
  (`_quillo_calls`), so copies sharing it answer a call one of them answered, or say it
  is still running, whichever copy a balancer sends it to. Use HTTPS (or a private
  network) between services so calls cannot be seen. The tenant goes with the call. Only `server fn`s are offered. Its routes, under its prefix, are passed on to it,
  but not Quillo's own addresses there (`/billing/_quillo/rpc/...`, `.../service/...`,
  `.../job/...`). Run alone, a service is a program served as one:
  `quillo run billing.qwe --base /billing --as-service` ([`--base`](#options-of-quillo-run): the path it is
  served under, which its links and redirects carry; [`--as-service`](#options-of-quillo-run): see below).
- `quillo run app.qwe --split` runs each service the app declares (and those they
  declare) as a process of its own, on 127.0.0.1 at a port of its own (a free one; the
  same again when it is restarted), its log lines marked with its name, all signed with
  one secret (32 random bytes from the system, unless `QUILLO_SERVICE_SECRET` is set,
  so a copy started by hand elsewhere cannot join a split app unless the secret is
  set); they stop with it (killed, not drained). Before any service starts, the app
  checks the whole tree once, its own files and every service's, and says each warning
  once; an error anywhere stops it there, before anything waits. Each is then started as
  `quillo run <file> --port P --as-service --service-name NAME`, with `--base`, [`--watch`](#options-of-quillo-run),
  [`--no-check`](#options-of-quillo-run), [`--no-build`](#options-of-quillo-run), [`--describe`](#options-of-quillo-run) and the [`--policy`](#options-of-quillo-run) files passed down, and
  [`--log`](#options-of-quillo-run), [`--workers`](#options-of-quillo-run), [`--max-body`](#options-of-quillo-run), [`--metrics`](#options-of-quillo-run), [`--no-jobs`](#options-of-quillo-run), [`--env-file`](#options-of-quillo-run) and
  `--service-url` not; it skips its own first check, which the app has made (a reload
  under `--watch` checks again). A service that is not up within 20 s stops the app with
  exit 1, and one whose process ends while starting is said at once: `service billing
  stopped at start (exit status: 1; see its lines above)`. Under `--watch` the app
  watches its services' files too, as in one process: a function added to `billing.qwe`
  reloads billing's process and the app, and the app calls it at once. A service whose process ends while the app runs is
  started again, the same way, after a second (twice as long each time it ends again
  soon, at most 30 s), with a line in the log. [`--scale risk=3`](#options-of-quillo-run) runs `risk` as three
  copies (up to 64), called in turn; they share its database, so give it one that
  several processes can open (SQLite on one machine, PostgreSQL across machines). Copies
  share only that: each has its own global variables, as [`--instances`](#options-of-quillo-run) do. A service's
  channels work apart too: pages connect to the app's process, which asks the services
  which channels their `channel.on` took, hands what pages send on one to its service
  (in order, a batch at a time, with the page's session), and passes on what the
  service broadcasts, sends or asks of `channel.peers` to its pages; all of it signed
  with `QUILLO_SERVICE_SECRET`, and none of it logged. Each crossing is a call on this
  machine (well under a millisecond): a game service ticking 20 times a second reaches
  its players 20 times a second. With `--scale`, channels go to the service's first copy
  only, so one copy holds a game's world.
- Either way, what crosses is data, as JSON: arguments and results are copied (a
  function arrives as `null`; a decimal stays a decimal, its scale and all), so a
  program behaves the same whichever way it runs. Optional arguments left out are not sent, so the service's own defaults apply. A
  call that cannot be made (the service not answering) is an error value: `?` passes
  it on, `??` replaces it.
- **Live data across services.** A page's `live` loader that gets its data from a service
  follows only what it reads itself: the service's tables and variables are the
  service's, and their changes wake no page of the caller. So such a loader `watch`es a
  topic, and the code that changes the data publishes it: the service itself
  (`publish("account:7")` in its `server fn`), or the caller after the call. A topic a
  service publishes while answering a call reaches the caller's pages in one process and
  split alike: it comes back with the answer, and the caller announces it to its own
  pages, through every service in between. What a service publishes on its own, not
  while answering a call (a call made later being delivered, a scheduled job, a channel
  handler), reaches the service's own pages: split apart, the app's pages see that
  change when they next load the data. `quillo check` warns about a loader that calls
  a service and watches nothing.

  ```quillo
  // accounts.qwe, the service
  server fn deposit(id: int, cents: int) {
      db.exec("update accounts set cents = cents + ? where id = ?", cents, id)
      publish(`account:${id}`)
  }

  // the app
  server fn load_balance(id: int) {
      watch(`account:${id}`)
      accounts.balance(id)
  }
  ```
- `quillo check` checks each call against the service's `server fn`s (their names,
  arguments and types; it may fail, so the result may be an error), the browser cannot
  call a service, and a call needs the `http` effect. Services may use one another. A
  service's name is a name of the program that declares it, so `service notify = ...`
  (or a file `notify.qwe`) hides the builtin `notify` there, with a warning; inside the
  service, a `server fn count` hides `count` the same way.
- **The service's file**, beside the app, is how the app knows what `billing.add` takes.
  As the app loads, it reads only the file's `server fn` headers (and those of the files
  it `use`s), so a mistake in one of its routes or bodies does not stop an app run with
  `--no-check` (`quillo check`, and [`--split`](#options-of-quillo-run), still check the file whole). A service
  that runs elsewhere need not be here at all: with no file and an address
  (`--service-url` or `QUILLO_SERVICE_BILLING`), the app asks the running service for its
  functions as it starts, as a call like any other (signed, each address in turn), and
  takes each one's header and contract from the answer. Only the functions this caller
  may call are given (a `for [...]` keeps the rest). A service that cannot be reached
  then stops the app: `billing.qwe is not here, and service billing did not give its
  functions: ...`. `quillo check` warns instead of failing: `service billing: billing.qwe
  is not here, so its calls are not checked; it must run elsewhere (QUILLO_SERVICE_BILLING
  or --service-url)`, and a call to it still needs the `http` effect.

  ```sh
  # on 10.0.0.5, with billing.qwe
  quillo run billing.qwe --port 8081 --as-service
  # elsewhere, with main.qwe and no billing.qwe
  quillo run main.qwe --service-url billing=http://10.0.0.5:8081
  ```
- Each service's transactions are its own: there is no transaction across services, and
  `db.atomic` spans databases of one program, not calls. A chain of calls commits step
  by step, so give each step a reference the caller makes up and keep what was done,
  so that a request sent again goes on from where it stopped rather than doing it
  twice. To undo the steps before one that fails, see [Sagas](#sagas).
- A service's `server fn`s are for other services: pages cannot call them. A service
  does not answer `/_quillo/rpc/...` (in one process, under its prefix; split, `--split`
  starts it with `--as-service`), only signed calls at `/_quillo/service/...`, whose
  arguments are checked against the parameter types all the way in, as pages' calls are.
- A program with `server fn`s and no routes is served too: a service may be only that.

When a service runs elsewhere, its calls are made to hold up:

- **Several addresses.** `--service-url billing=http://10.0.0.5:8081,http://10.0.0.6:8081`
  (or the same in `QUILLO_SERVICE_BILLING`): the same service on several servers, called
  in turn.
- **Copies that join** the app while it runs: see [Copies that join](#copies-that-join).
- **Ready.** `GET /_quillo/ready` answers 200 when what the program needs is there, 503
  when not, with each part as JSON: its database (`select 1`), each service it declares
  (one in its process asked in turn, down its own services; one elsewhere asked at each
  address, the answer kept two seconds), and checks of its own,
  `ready("ledger", () => len(accounts.problems() ?? ["down"]) == 0)`. Through the app,
  `/billing/_quillo/ready` asks a service served under `/billing`. `/_quillo/health` stays
  the process's own: up or not.
- **Contracts.** Each call to a service elsewhere carries its function's contract as the
  caller's copy of the service declares it (`assess(n: int, size?: int) -> map`); a
  service deployed apart whose own differs refuses the call and says both:
  ``service risk: 409 `assess` here is `assess(n: int, size: int) -> int`; the caller's
  copy of this service says `assess(n: int) -> int` ``.
- **Described** as OpenAPI, for a gateway: see [The program's API
  (OpenAPI)](#the-programs-api-openapi).
- **Tried again.** A call that got no connection, or a server answering 502, 503 or 504,
  is tried at the next address, a little later each time (100 ms, doubling, with some
  jitter). A call sent but not answered in time is tried again at the same address, with
  the same call id: the service remembers the calls it answered (for eleven minutes, up
  to 100,000 of them in memory, and in `_quillo_calls` when it has a database) and gives
  the same answer, so while the service stays up the function runs once however often
  the call is sent. A call still running there is waited for (the service says `503 this
  call is still running`, and the caller asks again). Only an answer of 200 is
  remembered, so a service that refused a call may take it after the next deploy.
- **Across a crash, at least once.** A service stopped in the middle of a function cannot
  say how far it got. The call is refused as still running for eleven minutes, as long as
  a signature lasts, and then runs again, though its first run may have committed. So a
  function that must not do its work twice keeps its own record: a reference the caller
  makes up as a key, or [`idempotent`](#idempotent-requests), which commits a record with
  the work.
- **Left alone when failing.** An address that fails several calls in a row is not
  called for a while (its circuit is open), then tried with one call; the others take its
  calls meanwhile. Each time it fails again it is left alone twice as long, up to
  sixteen times the cooldown. When every address is left alone, a call fails at once:
  an error value saying so.
- **Set where it is declared**, after the file: how long a try may take, how often a
  call is tried again, after how many failures in a row an address is left alone, and
  for how long (twice as long each time after):

  ```quillo
  service billing = "billing.qwe" { timeout: "2s", retries: 2, failures: 5, cooldown: "5s" }
  ```

  Those are the defaults, except `timeout` (10 s). Durations are `"500ms"`, `"2s"`,
  `"1m"` (`ms`, `s` and `m` only), or a number of milliseconds; `timeout` is more than
  0, `retries` 0 to 10, `failures` 1 or more. A try that runs out says so: `service
  billing: took more than 2 s (its timeout)`. They apply when a service runs elsewhere
  (`--split`, or at an address); in one process a call is a function's, which runs to
  its end.
- **A chain gives up together.** Each call carries the time its caller still waits for
  it, and the calls the service makes in turn wait no longer than that (their own
  `timeout` when it is shorter). So when the first link gives up, the ones below stop
  waiting too instead of finishing work nobody waits for; a call with no time left is
  not made. What a service is already doing runs to its end.
- **One id for a request, everywhere.** Each request has an id: the `x-request-id` it came
  with (a proxy's, a load balancer's), or one made for it. Its answer carries it, the
  JSON log shows it (`"rid"`), the code sees it (`request.id`), and every call to a
  service passes it on, so a request is followed across services. With `--log json`,
  each call to a service is a line of its own: the service, the function, the address,
  the status, how long it took, the tries and the id (a chain that failed altogether
  logs status 0). In a function a service answers with, `request.call` is the call's own
  id, the same for each try of it, and for a call made later the id `.later` gave.
- **On the wire**, for a balancer's rules, a log, or a service written in another
  language: a call is `POST <service>/_quillo/service/<fn>` with the arguments as a
  JSON array, and the headers `x-quillo-service: v=3,kid=<key's name>,t=<unix
  seconds>,sig=<hex>` (`kid` only for a key of the caller's own), `x-quillo-to` (the
  service's name), `x-quillo-call` (the call's id, up to 64 letters and digits),
  `x-quillo-caller`, `x-quillo-deadline` (milliseconds the caller still waits),
  `x-quillo-contract`, `x-quillo-tenant` and `x-request-id`, and `authorization` for a
  token and `x-quillo-forward-<name>` for each header `forward` passes on. The signature
  is HMAC-SHA256 under the key (`QUILLO_SERVICE_SECRET`'s first, or the caller's own) of
  `quillo-service-3`, the time, the service, the function, the tenant, the call id, the
  `kid`, the caller, the deadline and the contract, each on a line, then the body; the
  time must be within five minutes. Version 2 (`t=...,sig=...` over `quillo-service-2`,
  the time, the service, the function, the tenant and the call id) is still taken, with
  the caller's name then not known. The answer is `{"ok": value}` or `{"err": payload}`
  (with `"published": [...]`, the topics to announce), signed with the call's key in
  `x-quillo-answer: sig=<hex>`, HMAC-SHA256 of `quillo-answer-3`, the call id and the
  status, each on a line, then the body. It is 401 for a call that proves no caller (a
  bad or old signature, another service's, a key or token not taken), 403 for a caller
  `for` or `check` refuses, 404 for no such function, 409 for a contract that differs,
  503 while the call still runs. A server in another language that answers so,
  remembers call ids, and answers `GET /_quillo/ready`, is a service; its caller keeps a
  `.qwe` file with the `server fn` signatures for `quillo check` and the contract, or the
  service answers the call `_contracts` (`POST <service>/_quillo/service/_contracts`,
  signed as any call, no arguments) with `{"ok": [{"header": "server fn add(a: int, b:
  int) -> int = null", "contract": "add(a: int, b: int) -> int"}, ...]}`, as a Quillo
  service does for an app without its file. The routes under a service's prefix are passed on as they come, but not
  Quillo's own addresses there. A copy [joins](#copies-that-join) at `POST
  <app>/_quillo/join`, with `{ "service": "billing", "port": 8081, "leave": false }`
  (and `"url"` under [`--join-as`](#options-of-quillo-run)), signed as a call is to the service `_join` and the
  function `billing` (with its `QUILLO_SERVICE_KEY` as `kid=billing`, or under
  `QUILLO_SERVICE_SECRET`; a nonce in `x-quillo-call`, taken once). The answer is 200 `{
  service, at }`, the address the copy is recorded at, or 400 (no port, an address not
  known, a `url` that does not lead to where the copy joined from), 401 (a bad or old
  signature, no nonce, the shared secret where the app has the service's key, another
  service's key), 403 (the app has neither `QUILLO_SERVICE_SECRET` nor keys, or
  [`--no-join`](#options-of-quillo-run) and [`--allow-join`](#options-of-quillo-run) refuse it), 404 (no such service), 405 (not a POST) or
  409 (a nonce sent before). Only copies call it, so a proxy in front of the app need not
  pass it from outside.
- **Made later.** `billing.later.invoice(name, 1200)` does not wait: the call is kept, its
  id is the result, and it is made in the background, tried every second, then a little
  later each time (up to five minutes), until it gets through. Since the service
  remembers call ids, it runs once while it stays up. Checked as the call itself is
  (`quillo check`).
  - **It goes with the transaction.** The call is kept in the database the work is in,
    the program's or the tenant's (`_quillo_later`: see [Quillo's own
    tables](#standard-library-quillos-own-tables)), so inside `db.transaction` it commits or rolls back
    with the work, on every database. It outlives a restart, and servers sharing that
    database make each once; a program with no database keeps its calls in
    `quillo-system.db` next to it.
  - **What finishes it.** A value, or an error value the function gave, finishes a call;
    its answer is kept for a day. Any other answer does not: a 401, 404 or 409 from a
    deploy gone wrong, or a 500, is tried again for about a day, and a service that does
    not answer at all is waited for however long it takes. A 400, arguments the service
    refuses, cannot work and is given up on at once. A call given up on stays in
    `_quillo_later` with `tries` -1 and why in `error`, is counted by `--metrics`, and goes
    again with `update _quillo_later set at = 0, tries = 0 where tries < 0`.
  - **Asking after one.** `later.status(id)` gives `{ state: "waiting", tries, error }`,
    `{ state: "done", result }`, `{ state: "failed", error }`, or `null` if nothing is
    known of it: a call done over a day ago, kept in memory and done, or made in a
    transaction not committed yet. `result` is what the function gave, an error value
    if it said no. `error` is an error value, as it can name addresses, so a visitor
    is not shown it.
  - **Who delivers them.** The thread that also finishes `db.atomic`'s work, for each
    program in the process: the app and the services it runs, however the program runs.
    `--no-jobs` does not stop it. It takes 50 calls a go from each database, each second,
    and the next go at once after a full one. A call that has to wait does not hold up
    the next, so order is not kept.

See [services](https://github.com/quillolang/examples/tree/main/services): a shop that bills through a billing service.

### Authentication between services

A call signed with `QUILLO_SERVICE_SECRET` proves one thing: the caller holds the secret.
Every service holding it can sign as any other, call any function of any service, and name
any tenant, so the most exposed service in a system holds the key to all the others. A
caller can prove more than that, and a service can say whom it takes calls from.

The caller says how it proves who it is, where it declares the service:

```quillo
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY"), as: "shop" } }
```

| `auth` | The call |
|---|---|
| `"quillo"` (the default) | signed with the shared secret |
| `{ key, as }` | signed with a key of the caller's own, which the service knows it by: `as` is the name (the program's service name unless given). A key of `notify`'s cannot sign as `shop` |
| `{ bearer: f }` | `Authorization: Bearer <token>`, the token from `f` (a function, Quillo's or a Rust file's) at each call, or text |
| `{ oauth: { token_url, client_id, client_secret, scope, audience, method } }` | a token fetched by OAuth 2.0's client credentials grant (`method`: `"post"`, the client's secret in the form, or `"basic"`, in `Authorization`), kept until 30 s before it expires, and fetched again once when the service answers 401 |
| `{ mtls: { cert, key, ca } }` | a client certificate on the connection (PEM files; `ca` a file or `"system"`), alone or beside one of the others |

A call that proves itself with a token (`bearer`, `oauth`) is not also signed with the shared
secret, so the service checks the token, not the secret.

```quillo
service billing = "billing.qwe" { auth: { oauth: {
    token_url: env("IDP_TOKEN_URL"), client_id: "shop", client_secret: env("SHOP_CLIENT_SECRET"),
    scope: "billing", audience: "billing",
} }, forward: ["authorization"] }
```

`forward: ["authorization"]` passes the visitor's own headers of the request the call is
made in along with it; the service reads them in `request.forwarded`. A call signed with a key
or the secret signs them too, so they cannot be changed on the way; they are still what the
visitor sent. `quillo check` warns of a `key` or `client_secret`
written in the source, where every copy of the program has it (``service billing: its
`key` is written in the source, ... read it with env("...")``), and of a misspelt option
(`auht`: did you mean `auth`?). A wrong value stops the program as it loads, saying what
is wrong (`` service billing: `oauth` needs `client_id` ``).

The service says what it takes, once, at its top level:

```quillo
// billing.qwe
accept_calls {
    keys: { shop: [env("SHOP_KEY"), env("SHOP_KEY_OLD")], orders: env("ORDERS_KEY") },
    jwt: { issuer: env("IDP"), audience: "billing", jwks: env("IDP") + "/.well-known/jwks.json" },
    client_cert: { header: "x-forwarded-client-cert" },
    check: (caller) => caller.name != "mallory",
}

server fn refund(account: int, cents: int) for ["shop"] {
    // request.caller: { name: "shop", by: "key" }
    { refunded: cents }
}
```

- `keys`: a key for each caller, by name; a list takes any of its keys, while one is
  changed.
- `shared: false`: calls signed with the shared secret are refused (`this service takes
  no calls signed with the shared secret`). Without it they are taken as before.
- `jwt: { issuer, audience, jwks, secret }`: a bearer token, an OAuth 2.0 access token,
  checked against the identity provider's keys at `jwks` (a URL, kept for ten minutes,
  fetched again at most once a minute for a key it does not have: RS256, ES256, EdDSA)
  or against `secret` (HS256). `audience` is required, so a token meant for another
  service is not taken here; `exp` is required too, and `nbf`, `iss` and `aud` are
  checked, with a minute's leeway. A token that fails is a 401 that says why (`the
  bearer token: the token is not for billing (aud)`, `... has expired`, `... has no exp:
  it would be good for ever`, `an unsigned token`).
- `client_cert: { header }`: the identity in the header a proxy that ended mutual TLS
  sets, as Envoy's `x-forwarded-client-cert` (`Subject="CN=shop,O=Acme";URI=spiffe://...`):
  the SPIFFE id's last part, else the subject's CN. Quillo ends no TLS itself, so the
  header is believed only from an address the option [`--trust-proxy`](#options-of-quillo-run) names
  (`quillo run billing.qwe --as-service --trust-proxy 10.0.0.2`); from anyone else it is
  a 401.
- `check: (caller) => ...`, run with `request.caller` before the call's arguments are
  read: `false` refuses it with 403 (and an error value, with its text), a map takes the
  caller's place, and `reply` answers the call there.
- An `accept_calls` with nothing in it is an error, and so is an option it does not have.

`server fn refund(...) for ["shop"]` takes calls from those callers only, checked before
the arguments are read: another gets 403 (`` `refund` takes calls from shop only; this is
`notify` ``). `quillo check` refuses `for []`, `for [shop]` (a caller is named in plain
text, `"shop"`) and `for "shop"`, and `quillo docs` puts each function's callers in
`x-quillo-callers`, for a gateway to enforce too.

What the function sees, where a service runs elsewhere (in one process a call is a
function's call: `request.caller` is null, and `for` is not checked):

- `request.caller`: `{ name, by }`, `by` being `"quillo"` (the shared secret; `name` as
  the caller signed it), `"key"`, `"jwt"` (with `scopes`, from `scope` or `scp`, and the
  token's `claims`; `name` its `client_id`, `azp` or `sub`) or `"mtls"` (with `subject`,
  and `uri` for a SPIFFE id). What `check` gave, when it gave a map.
- `request.user`: the claims of the visitor's token, when the caller forwarded its
  `authorization` and `accept_calls`'s `jwt` takes it; else null.
- `request.forwarded`: the headers forwarded, by name.

A call proven several ways is taken by the first that holds, in this order: a key of the
caller's own, the shared secret, a token, a certificate. A caller with `QUILLO_SERVICE_SECRET`
set signs every call with it, a bearer one too, so a service that is to check tokens says
`shared: false`.

Answers are signed back with the key the call was signed with, and a caller that signs
with a key of its own takes no answer without it (``service billing: the answer from
http://... is not signed by the service``), so a copy that only says it is the service
cannot answer for it. Signatures are now version 3, which also covers the caller's name,
the deadline and the contract; services still take version 2, so upgrade the services
before their callers.

The shared secret is changed without stopping anything: `QUILLO_SERVICE_SECRET=new,old`
(or `QUILLO_SERVICE_SECRET=new` and `QUILLO_SERVICE_SECRET_PREVIOUS=old`) signs with the
first and takes any. Set both on every machine, then drop the old one. `keys` take a list
for the same.

For a caller that is not a Quillo service (a gateway, a program in another language with
an OAuth token), `accept_calls { jwt }` takes its token at `/_quillo/service/<fn>` with
`x-quillo-to` and `x-quillo-call`. Or the service offers routes, a JSON API, checked as
middleware with the catalog's `jwt.qwer` (2.0.0: `jwt_check(token, secret, { audience,
issuer, require })`, `exp` required unless told otherwise, and `jwt_verify_jwks` for an
identity provider's keys) or `oidc.qwer` (1.1.0: `oidc_client_credentials` fetches a
token, `oidc_verify_access` checks one), and called with `http.*`. That gives up what
`service` brings: `quillo check` of each call, contracts, retries run once, deadlines,
`.later` and published topics.

```quillo
use "jwt.qwer"

route POST /api/refund {
    let token = replace(request.headers.authorization ?? "", "Bearer ", "")
    let claims = jwt_check(token, env("JWT_KEY"), { audience: "orders", issuer: "idp" })
    if is_err(claims) { reply 401 { error: error(claims) } }
    { refunded: request.json.cents, for: claims.sub }
}
```

### Copies that join

`quillo run billing.qwe --as-service --port 8081 --join http://app:8080` starts one more
copy of `billing` that joins the app while it runs: it says so, signed, every 5 s, and that
it is leaving when stopped (the app drops one it has not heard from in 15 s). Its address
is the one it connects from, with its [`--port`](#options-of-quillo-run), called over `http://`; `--join-as
https://billing-3.internal:8443` gives the address it is called at instead, which must
lead to where it joined from.

- A copy signs with `QUILLO_SERVICE_KEY`, billing's own key, when it is set (`kid` is
  its service's name), else with `QUILLO_SERVICE_SECRET`. An app whose `accept_calls`
  has a key for `billing` takes copies of `billing` only so: the shared secret is not
  enough (``this app takes copies of `billing` signed with its key (QUILLO_SERVICE_KEY on
  the copy)``), and a key of another service's never is. Without such a key, the secret
  is, as before: anyone with it can join as any service the app declares, and take its
  calls.
- [`--no-join`](#options-of-quillo-run) takes no copies, and [`--allow-join billing,risk`](#options-of-quillo-run) only those (403 to the
  others).

- When `billing` runs in the app's process, the copies that joined take every call, in
  turn, and the app runs a call itself only when no copy answers it: a monolith takes on
  copies as it gets busy, without a line of either changed.
- When the app was given addresses ([`--service-url`](#options-of-quillo-run), `QUILLO_SERVICE_<NAME>`), the given
  addresses and the joined copies are all taken in turn.
- Each copy has a circuit of its own. With several app servers, a copy joins each.
- Join the app's own address, not its public one: through a proxy the app sees the
  proxy's address, unless it runs with [`--trust-proxy`](#options-of-quillo-run) naming it. A copy behind NAT is
  recorded at an address the app cannot reach, so put such a copy behind a balancer
  instead: limer's `discover` pool with [`--register`](#options-of-quillo-run), keeping a call on one copy
  (`lb_policy hash header x-quillo-call`) or giving the copies a database to share, so a
  call sent again is not run again.

Services are found by registration: a copy joins the app ([`--join`](#options-of-quillo-run)), and any server joins
limer's pool (`--register`). A caller does not look a service up by name, so a service the
app does not run itself needs `--service-url` or `QUILLO_SERVICE_<NAME>`, and
`QUILLO_SERVICE_SECRET` is set on every machine by hand.

### The program's API (OpenAPI)

`quillo docs app.qwe` writes the program's API as OpenAPI 3, from its source: each
route's path, query and payload patterns, and each `server fn`'s contract
(`x-quillo-server-fns`, with `x-quillo-callers` for one declared `for [...]`), with the routes of the services it serves under their prefixes.
Started with the option [`--describe`](#options-of-quillo-run), `quillo run app.qwe --describe`, the server serves
the same at `GET /_quillo/describe`, for a gateway to import. A prefixed service's `server fn`s are not in the app's document: they are called by
other services, signed, not at `/billing/_quillo/rpc`. Under [`--split`](#options-of-quillo-run) the app's document
still holds the services' routes, read from their source, and `--describe` is passed down,
so `/billing/_quillo/describe` answers the service's own document, its paths without the
prefix.

What the document holds:

- Its title is the program file's name (`main`), both ways. Under [`--base /shop`](#options-of-quillo-run) it has
  `"servers": [{ "url": "/shop" }]`.
- Several routes on one method and path shape are one operation: their bodies a `oneOf`
  (each literal field an `enum`), their query parameters merged (required only where every
  route requires them), the path template named as the first route names it (`/users/{id}`,
  its parameter a `oneOf` of the routes' types), and `x-quillo-at` a list of the places
  when there are several.
- A rest parameter (`/files/*path`) is `/files/{path}`, with `"x-quillo-rest": true` and
  the description "the rest of the path, slashes included": an OpenAPI template matches one
  segment, so a gateway routes the rest itself.
- Each operation has one response, 200, with no content schema.

`/_quillo/describe`, when on, is public: it lists every route, those no page links to as
well, each `server fn`'s signature, the file and line of every route, and the version of
Quillo. Keep it private as `/_quillo/metrics` is, with a rule at the proxy, or do not turn
it on in production.

### Calling another Quillo site

`site shop = "https://shop.example.com"` names a deployed Quillo site, and `shop.notes(a,
b)` calls its `server fn notes` as the site's own pages do: `POST
<url>/_quillo/rpc/notes` with the arguments as a JSON array. It gives the answer's value,
the site's error as an error value, or an error value when the site cannot be reached.
Decimals and big ints cross as they do for pages. The address is any expression:

```quillo
// shop.qwe, deployed at https://shop.example.com
server fn notes(n: int) = range(1, n + 1) | map((i) => { id: i, text: `note ${i}` })

// main.qwe, elsewhere
site shop = env("SHOP_URL", "https://shop.example.com")

server fn my_notes() = shop.notes(10)

route GET / = <ul>{for n in my_notes() ?? [] => <li>{n.text}</li>}</ul>
```

`SHOP_URL=http://127.0.0.1:8081 quillo run main.qwe` with the shop on that port lists
`note 1` to `note 10`; with the shop down, `my_notes()` is an error value and the list is
empty.

- In an app of one person (a phone app, `quillo app`, or with `QUILLO_SITE_COOKIES=1`),
  the cookies the site sets are kept in `.quillo/sites/<name>.json` in the program's
  folder, so whoever signed in stays signed in. On a server they are not kept (a log line
  says so, once), as one visitor's session would be every visitor's.
- It runs on the server side only: `quillo check` refuses `shop.x()` in a component
  ("call it in a server fn"). `site` stays an ordinary name elsewhere (`let site = ...`).
- `/_quillo/rpc/<fn>` is a stable wire for programs: a JSON array of arguments in,
  `{"ok": value}` or `{"err": error}` out. A program needs no `Origin` header; another
  site's `Origin` is refused.
- A phone app runs its services inside the app. To use a server's, call the server with
  `site`, not `service`.

### Sagas

A saga is a business operation spread over services that share no transaction. Each
service commits its own part, and if a later part cannot happen, the earlier parts are
undone by compensating steps you write: an order reserves stock, charges a card and books
a parcel, and if the parcel cannot be booked, the charge is refunded and the stock
released.

Quillo has no `saga` word. One is a table, a job that moves each row a step at a time, and
ordinary calls, and it needs nothing else. The state is written before anything else
happens, so a server that stops goes on from there:

```quillo
db.open("orders.db")
db.exec("create table if not exists sagas (id text primary key, cents int not null, state text not null, step int not null, tries int not null, next_at bigint not null, why text)")

service inventory = "inventory.qwe" { timeout: "1s", retries: 1 }
service payments = "payments.qwe" { timeout: "1s", retries: 1 }

let steps = 2

fn forward(s) = match s.step {
    0 => inventory.reserve(s.id)
    _ => payments.charge(s.id, s.cents)
}

fn undo(s) = match s.step {
    0 => inventory.release(s.id)
    _ => payments.refund(s.id)
}

fn move(s, state, step, why) =
    db.exec("update sagas set state = ?, step = ?, tries = 0, next_at = 0, why = ? where id = ? and state = ? and step = ?", state, step, why, s.id, s.state, s.step)

fn try_again(s, why) =
    db.exec("update sagas set tries = ?, next_at = ?, why = ? where id = ?", s.tries + 1, now() + min([1000 * pow(2, s.tries + 1), 60000]), why, s.id)

fn advance(s) {
    if s.state == "running" {
        let r = forward(s)
        if is_err(r) {
            // No answer: it may have run. Try again, then undo this step too.
            if s.tries + 1 >= 3 {
                move(s, "undoing", s.step, str(r))
            } else {
                try_again(s, str(r))
            }
        } else if !r.ok {
            move(s, "undoing", s.step - 1, r.why)
        } else if s.step + 1 == steps {
            move(s, "done", s.step, null)
        } else {
            move(s, "running", s.step + 1, null)
        }
    } else if s.step < 0 {
        move(s, "undone", s.step, s.why)
    } else {
        let r = undo(s)
        if is_err(r) {
            // An undo is never given up on: it waits for the service.
            try_again(s, str(r))
        } else {
            move(s, "undoing", s.step - 1, s.why)
        }
    }
}

fn advance_all() {
    for s in db.query("select * from sagas where state in ('running', 'undoing') and next_at <= ? order by id limit 100", now()) {
        advance(s)
    }
}

schedule("1s", advance_all)
```

And a step, the same in every service:

```quillo
server fn charge(ref: str, cents: int) -> map {
    db.transaction(() => {
        let was = db.one("select state from charges where ref = ?", ref)
        if was != null {
            return { ok: was.state == "charged", why: was.state }
        }
        if cents > 100000 {
            return { ok: false, why: "declined" }
        }
        db.exec("insert into charges (ref, cents, state) values (?, ?, 'charged')", ref, cents)
        { ok: true }
    })
}

server fn refund(ref: str) -> map {
    db.exec("insert into charges (ref, cents, state) values (?, 0, 'refunded') on conflict (ref) do update set state = 'refunded'", ref)
    { ok: true }
}
```

What starts a saga is a row: the order's route writes it and answers at once, and the
job does the rest. The visitor follows it on a page that reads the row:

```quillo
route GET /order =
    <form method="post" action="/order">
        <input type="number" name="cents" value="2500"/>
        <button>Order</button>
    </form>

route POST /order { form: { cents: int } } {
    let id = uuid()
    db.exec("insert into sagas (id, cents, state, step, tries, next_at) values (?, ?, 'running', 0, 0, 0)", id, cents)
    redirect(`/order/${id}`)
}

route GET /order/:id {
    let s = db.one("select state, why from sagas where id = ?", id) ?? reply 404 "No such order"
    match s.state {
        "done" => <p>Your order is placed.</p>
        "undone" => <p>Your order could not be placed ({s.why}); nothing was charged.</p>
        _ => <p>Placing your order… <a href={`/order/${id}`}>See how it is going</a></p>
    }
}
```

An order of 2,500 shows "Placing your order…", then, a couple of seconds later, "Your
order is placed." One of 200,000 is declined at the charge: the stock is released, and
the page says `Your order could not be placed (declined); nothing was charged.`

Five rules carry it, and leaving out any one breaks it:

1. **The reference is the key.** The caller makes it up, and a step keyed on it changes
   nothing the second time. A job's tries are new calls with new ids, so `request.call`
   and `idempotent` do not do this job here, though they do for one call sent again.
2. **A "no" is an answer, not an error.** A step gives `{ ok: false, why }`. An error value
   means no answer came and the step may have run, which is a different decision.
3. **An undo works on what never happened, and leaves a mark.** A refund with no charge
   writes `refunded`, so a charge that arrives late is refused. A call that times out
   keeps running at the service, so this order really happens.
4. **A step with no answer is undone too.** It may have run, and rule 3 makes that safe.
5. **An undo never gives up.** It waits for the service. If it can never work, a person has
   to look, so a saga long in `undoing` is what to alert on.

Put the step that cannot be undone last. And sagas do not isolate each other: between
steps, other requests see the stock reserved and the charge taken, so a state such as
`held` must mean something to them.

`.later` can carry a step too, committing the move with the call: `later.status(id)` tells
how it ended, and a `server fn` with `idempotent` is safe across a crash. A job as above
stays the simpler choice when the next move depends on the answer.
