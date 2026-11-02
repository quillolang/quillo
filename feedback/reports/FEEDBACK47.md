# Feedback from probing `route` and `routes` groups: middleware that lets everything through, patterns that swallow it, and paths read two ways

| | |
|---|---|
| App | None: probe programs aimed at `route` and `routes` groups. They cover the route line's grammar, paths and typed parameters, query, form and JSON patterns, group middleware and request patterns, `with` on groups, and what a group does not cover (server fns, `site_headers`, the server's own 400/405/OPTIONS answers) |
| Quillo | 0.6.0, release build of the runtime-coverage branch's working tree (it reports `54dd1bd`). Every function named below is the same on main `168a462` (read with `git show`, not built): `parse_group`, `answer_with`, `match_path`, `site_headers_into` and `coerce` |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`, `quillo docs`, and `quillo run` with `curl` (with `--path-as-is` for raw paths) and `wrk`, in the session's scratch folder. No browser run. Read: [src/parser.rs](../../src/parser.rs) (`parse_route`, `parse_path`, `parse_route_rest`, `group_ahead`, `parse_group`, `parse_pattern_alts`), [src/server.rs](../../src/server.rs) (`match_path`, the route loop, `site_headers_into`), [src/builtins.rs](../../src/builtins.rs) (`answer_with`), [src/interp.rs](../../src/interp.rs) (`coerce`), USERGUIDE.md (Routes, Route groups, What a route returns, What a route cannot do, Effects) |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section, and the scripts are at the end. Numbers start at Q-534, after FEEDBACK46.md's Q-533.

Four of the issues let a request past a group's middleware or a path's headers: Q-534 to Q-537. `quillo check` says `ok` to all four. Q-538 and Q-539 are what a guarded group does not cover, though a reader of the guide would expect it to. The rest are paths, patterns and messages.

| ID | In short | Kind |
|---|---|---|
| Q-534 | Group middleware lets the route run unless it gives a response: `false`, `err(...)`, text and `null` all let it through | silent, security |
| Q-535 | A pattern that binds `request` (`{ json: { request: map } }`) replaces what the group's middleware is given, so a JSON body can forge the headers it checks | silent, security |
| Q-536 | `routes /api { headers: ... } \| auth { ... }` reads the `\|` as an or-pattern: the request pattern always matches, and the middleware is never called | silent, security |
| Q-537 | `site_headers("/admin", ...)` compares the raw path, and routes compare its decoded pieces: `//admin/users` and `/%61dmin/users` get the page without its headers | silent, security |
| Q-538 | The `server fn`s a guarded page calls are not behind the group's middleware, and the groups section does not say so | docs, silent |
| Q-539 | A guarded group answers 400, 404, 405 and `OPTIONS` before its middleware runs, so anyone can map its routes, methods and parameter types | silent |
| Q-540 | Anything after `*rest` is ignored: `routes /docs/*path { route GET /edit ... route GET /view ... }` sends `/docs/a/view` to `edit` | silent |
| Q-541 | A route that can never be reached passes `check`: one after the same route with no pattern, `route HEAD`, routes under `/_quillo/` | silent |
| Q-542 | Typed parameters take `NaN`, `inf` and `1e999` as floats (sent as JSON `null`), `+5`, `05` and `%205` as ints; `:x(str)` trims and `:x` does not | silent |
| Q-543 | Form and query values meet patterns unevenly: a blank field (`?page=`) with a default is a 400, `tag[]=a` gives `[]` silently, `bool` takes `yes` but not `TRUE` | loud, silent |
| Q-544 | Every pattern miss is `bad request: no route pattern matches this request`, under `--watch` too, and nothing is logged | loud |
| Q-545 | A host pattern compares the header as sent: `API.example.com`, `api.example.com:443` and `api.example.com.` miss | loud |
| Q-546 | Mistakes on the route line get other constructs' messages, or none | loud |

## New issues

### Q-534. Group middleware lets the route run unless it gives a response: `false`, `err(...)`, text and `null` all let it through

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | the protected route answers 200 to a request the middleware meant to refuse | 7 shapes of middleware |

```quillo
fn is_admin(r) = r.headers["x-role"] == "admin"                              // a predicate
fn deny_text(r) = r.headers["x-role"] == "admin" ? r : "admins only"
fn deny_err(r) = r.headers["x-role"] == "admin" ? r : err("admins only")
fn deny_null(r) = r.headers["x-role"] == "admin" ? r : null
fn deny_list(r) = r.headers["x-role"] == "admin" ? r : [403, "no"]
fn ok_respond(r) = r.headers["x-role"] == "admin" ? r : respond(403, "no")

routes /a | is_admin { route GET /x = "A: secret" }
routes /b | deny_text { route GET /x = "B: secret" }
routes /c | deny_err { route GET /x = "C: secret" }
routes /e | deny_null { route GET /x = "E: secret" }
routes /f | deny_list { route GET /x = "F: secret" }
routes /g | ok_respond { route GET /x = "G: secret" }
```

```
$ quillo check main.qwe
main.qwe: ok (6 routes, 6 functions, 0 components)
$ for g in a b c e f g; do curl -s localhost:4711/$g/x; echo; done
A: secret        200
B: secret        200
C: secret        200
E: secret        200
F: secret        200
no               403
```

Only a response stops the request: `reply`, `respond(...)` or `redirect(...)`. The guide says so ("anything else it gives lets the route go on"), so this is by design. But it is fail-open, and the shapes that fail are the ones developers write:

- **A predicate** (`fn is_admin(r) = ...`): the most natural middleware there is. Its `false` lets everyone in.
- **`err("admins only")`**: the language's own way to say "no". Everywhere else, a route that gives an `err` answers 500 with the message. Here it lets the route run.
- **Text**: a route answers with text, so a middleware that gives `"admins only"` reads as an answer.

The cause is `answer_with` ([builtins.rs:2632](../../src/builtins.rs#L2632)). It turns a `Value::Response` into a reply and every other value into `Ok(Nil)`. `parse_group` wraps each middleware call in it ([parser.rs:1917-1918](../../src/parser.rs#L1917-L1918)).

**Suggestion:** fail closed. Let the route go on only when the middleware gives back the request it was given, or `null` (`admins` in tests/routes.rs gives `null`). A `Response` answers, as now. `false` and an error value answer 403 and 500. Text and anything else would be a runtime error naming the middleware ("`is_admin` gave a bool: middleware gives the request back to go on, or a response to answer"). `quillo check` can do most of this from the types it knows: a middleware whose type is `bool`, `str` or `! E` is an error at the `|` line.

### Q-535. A pattern that binds `request` replaces what the group's middleware is given, so a JSON body can forge the headers it checks

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | the protected route answers 200 to a request without the token | 4 places the name can come from |

```quillo
fn signed_in(r) = r.headers.authorization == "Bearer s3cret" ? r : reply 401 "sign in"

routes /api | signed_in {
    // an approval API: the body carries the request being approved
    route POST /approve { json: { request: map } } = `approved ${request.id}`
    route POST /whole { json: request } = "whole secret"
}
```

```
$ quillo check main.qwe
main.qwe: ok (2 routes, 1 function, 0 components)
$ curl -s -H 'content-type: application/json' -d '{"request":{"id":7}}' localhost:4714/api/approve
sign in                                                                     401
$ curl -s -H 'content-type: application/json' \
    -d '{"request":{"id":7,"headers":{"authorization":"Bearer s3cret"}}}' localhost:4714/api/approve
approved 7                                                                  200
$ curl -s -H 'content-type: application/json' \
    -d '{"headers":{"authorization":"Bearer s3cret"}}' localhost:4714/api/whole
whole secret                                                                200
```

No token was sent. The middleware read `r.headers.authorization` from the visitor's JSON. A path parameter `/:request` or a query field `? { request: str }` hands the middleware a string, which is a 500 (`str has no field headers`). That fails closed, but it is still broken.

The cause is where the middleware is called. `parse_group` makes each call `__answer(m(request))` and puts it at the top of the route's body ([parser.rs:1917](../../src/parser.rs#L1917)). The body runs in the route's scope, after the path, query and request patterns have bound their names. So `request` is whatever the route's patterns made of that name, and the middleware's own name is looked up there too. `check` catches the second case for a path parameter (`/:guard` with `| guard` gives "cannot call `guard`: it is str"), but not a pattern field called `request`.

`request` is a common field name in JSON APIs (approvals, webhooks that carry the request they answer, audit records). A developer writing `{ json: { request: map } }` has no reason to think of the group around the route.

**Suggestion:** call the middleware with the request itself, not the name `request` in the route's scope: a hidden name the patterns cannot bind (`__request`), or the call made before the patterns bind. Separately, a pattern that binds `request` in any route deserves a `check` error ("`request` is the request: name this field `req` with `{ json: { request: req } }`"), since in the route's own body it also hides the request.

### Q-536. `routes /api { headers: ... } | auth { ... }` reads the `|` as an or-pattern: the request pattern always matches, and the middleware is never called

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | the group's routes answer 200 to requests that match neither the pattern nor the middleware | groups and a single route |

The guide's order is middleware first, then the pattern (`routes /admin | signed_in { pattern } { ... }`). Written the other way round:

```quillo
fn g(r) = reply 401 "no"
routes /api { headers: { "x-key": "k1" } } | g {
    route GET /x = `api x, g is ${json.stringify(g.headers.host)}`
}
route GET /r { headers: { "x-key": "k1" } } | g = "route secret"
```

```
$ quillo check main.qwe
main.qwe: ok (3 routes, 1 function, 0 components)
$ curl -s localhost:4717/api/x
api x, g is "localhost:4717"            200
$ curl -s localhost:4717/r
route secret                            200
```

No `x-key` was sent, and `g` always refuses. `g` is now the whole request: `{ headers: ... } | g` parsed as a pattern with two alternatives, the second a name that binds anything ([parser.rs:2768-2771](../../src/parser.rs#L2768-L2771), `parse_pattern_alts`). `parse_group` then sees `{` after it and takes the or-pattern as the group's request pattern ([parser.rs:1886-1891](../../src/parser.rs#L1886-L1891)). The same happens on a route line, where `| g` might be written in the hope of a route's own middleware. `quillo docs` lists `/api/x` with nothing to show either was meant.

Nothing in the result looks wrong until someone tries a request that should fail. The function `g` is also hidden by a pattern variable of the same name, with no warning.

**Suggestion:** in a route's or a group's request pattern, refuse an alternative that is a bare name ("`| g` after a request pattern: a group's middleware goes before the pattern, `routes /api | g { headers: ... } { ... }`; a route has no middleware of its own"). More generally, `check` could warn when a pattern variable hides a function of the file.

### Q-537. `site_headers("/admin", ...)` compares the raw path, and routes compare its decoded pieces: `//admin/users` and `/%61dmin/users` get the page without its headers

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | the admin page served with no `content-security-policy` | 9 spellings of the path |

```quillo
site_headers("/admin", { "content-security-policy": "frame-ancestors 'none'", "x-admin": "yes" })
routes /admin {
    route GET /users = <html><body>users</body></html>
}
```

| Path sent (`curl --path-as-is`) | Status | `site_headers("/admin")` applied |
|---|---|---|
| `/admin/users` | 200 | yes |
| `/admin//users`, `/admin/users/` | 200 | yes |
| `//admin/users` | 200 | **no** (only the default `x-frame-options: SAMEORIGIN`) |
| `/%61dmin/users` | 200 | **no** |
| `/Admin/users`, `/admin%2Fusers`, `/./admin/users` | 404 | (no route) |

Routes split the path at `/`, drop empty pieces and percent-decode each piece ([server.rs:133](../../src/server.rs#L133), `match_path`). `site_headers_into` tests the path as it came, with `starts_with` ([server.rs:765](../../src/server.rs#L765)). Browsers keep both `https://site//admin/users` and `%61` as written. So a page can frame `https://site.example//admin/users` without the `frame-ancestors 'none'` the program set for its admin area. In the same way, a `script-src` policy can be dropped where an injection would need it gone, and a `cache-control: no-store` too. Middleware that reads `r.path` to decide (`starts_with(r.path, "/admin/settings")`) is fooled the same way. Nothing gives the path as routes read it, so that code has nothing better to read.

**Suggestion:** match `site_headers` paths on the same pieces routes use (split, empty pieces dropped, decoded), so that a rule applies to every request whose route is under its path. Add the normalized path to the request (`request.route_path`, or the route's pattern as `request.route`) for middleware to read. Redirecting `//admin/users` to `/admin/users` would also be reasonable.

### Q-538. The `server fn`s a guarded page calls are not behind the group's middleware, and the groups section does not say so

| Kind | Detected by | Attempts |
|---|---|---|
| docs, silent | the admin page's data and its ban action, called without the admin header | 1 |

```quillo
fn admins(r) = r.headers["x-role"] == "admin" ? r : reply 403 "admins only"

server fn all_users() = [{ name: "ana", email: "ana@example.com" }]
server fn ban(name: str) = `banned ${name}`

component Users() {
    state users = all_users()
    <ul>{for u in users => <li>{u.email} <button @click={() => ban(u.name)}>ban</button></li>}</ul>
}

routes /admin | admins {
    route GET /users = <html><body><Users/></body></html>
}
```

```
$ curl -s localhost:4715/admin/users
admins only                                                  403
$ curl -s -X POST -H 'content-type: application/json' -d '[]' localhost:4715/_quillo/rpc/all_users
{"ok":[{"name":"ana","email":"ana@example.com"}]}            200
$ curl -s -X POST -H 'content-type: application/json' -d '["ana"]' localhost:4715/_quillo/rpc/ban
{"ok":"banned ana"}                                          200
```

The Server functions section does say that each call is a `POST /_quillo/rpc/<name>` and that a server fn guards itself (`request | auth.required | ...`). But the Route groups section compares groups to Laravel's `Route::prefix()->middleware()->group()` and says nothing about server fns. A developer who guards `/admin` with a group sees the page refuse strangers and stops there. Everything the page loads and every action it takes is open: the page's own `state` loaders and its buttons, and `live` loaders too, which are server fns as well.

**Suggestion:** say it in the groups section, beside the Laravel comparison: "a group guards its routes, not the `server fn`s their pages call; guard those in the function". Better, have `check` warn when a server fn is called only from components that are rendered only by routes behind group middleware, and the function has no middleware of its own. Best, let a server fn be declared inside a group (`routes /admin | admins { server fn ban(...) }`), so that it takes the group's middleware and `with`.

### Q-539. A guarded group answers 400, 404, 405 and `OPTIONS` before its middleware runs, so anyone can map its routes, methods and parameter types

| Kind | Detected by | Attempts |
|---|---|---|
| silent | no credentials sent; statuses and `Allow` headers compared | 15 requests |

```quillo
fn g(r) = r.headers["x-ok"] == "1" ? r : reply 401 "no"
routes /admin | g {
    route GET /users ? { page: int } = `users page ${page}`
    route POST /users { json: { name: str } } = `made ${name}`
    route DELETE /users/:id(int) = `deleted ${id}`
}
```

| Request, no credentials | Answer |
|---|---|
| `GET /admin/nothing` | 404 |
| `GET /admin/users` | 400 (the route exists, `page` is required) |
| `GET /admin/users?page=1` | 401 |
| `PUT /admin/users` | 405, `allow: GET, HEAD, OPTIONS, POST` |
| `OPTIONS /admin/users/7` | 204, `allow: DELETE, OPTIONS` |
| `DELETE /admin/users/abc` | 404 (so `:id` is a number) |
| `POST /admin/users` with `{}` | 400; with `{"name":"x"}`: 401 (the body's fields can be guessed one by one) |

Only the requests answered 401 reached the middleware (a counter in `g` showed it). It runs inside the route's body, so only after a route has matched on its method, its path and all its patterns. Every other answer comes from the server's route loop, before any middleware ([server.rs:656-660](../../src/server.rs#L656-L660)). Laravel does the same for 404 and 405. Express's `router.use(auth)` and nginx's `location /admin { auth_request ...; }` do not: there, everything under the prefix goes through the check first. The guide's comparison with Laravel leaves a developer unsure which applies.

The workaround is a last `route ANY /*rest = respond(404, "")` inside the group. That runs the middleware for everything under `/admin`. It is not in the guide.

**Suggestion:** when a request's path is under a group that has middleware, and no route of the group answers, run the group's middleware before answering 400, 404, 405 or the server's `OPTIONS`. Keep the status if it lets the request on. At the least, the groups section should state the order and show the `ANY /*rest` line.

### Q-540. Anything after `*rest` is ignored: `routes /docs/*path { route GET /edit ... route GET /view ... }` sends `/docs/a/view` to `edit`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | which route answers | a route, a group, two rests |

```quillo
route GET /f/*rest/more = `more rest=${rest}`
routes /docs/*path {
    route GET /edit = `edit ${path}`
    route GET /view = `view ${path}`
}
route GET /two/*a/*b = `a=${a} b=${b}`
```

```
$ quillo check main.qwe
main.qwe: ok (4 routes, 0 functions, 0 components)
/f/a            more rest=a          200    (no "more" in the path)
/f              more rest=           200
/docs/a/view    edit a/view          200    (the edit route)
/docs/a         edit a               200
/two/1/2/3      internal error       500    (b is never bound)
```

`match_path` returns as soon as it meets a rest piece ([server.rs:137-140](../../src/server.rs#L137-L140)), so whatever was written after it never counts. In a group, the prefix's `*path` makes the group's first route take every path under it. `parse_path` accepts a rest piece anywhere ([parser.rs:1701-1705](../../src/parser.rs#L1701-L1705)). Wanting `/docs/*path/edit` is reasonable, since that is how GitHub's `/blob/*path` and many file browsers work. But Quillo neither matches it nor refuses it.

**Suggestion:** make it a parse error for now: "`*path` takes the rest of the path, so it is last: nothing can follow it, in the route or its group's routes". Matching a rest in the middle (shortest or longest piece count) could come later, if wanted.

### Q-541. A route that can never be reached passes `check`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check` on each, then the server | 4 shapes |

```quillo
route GET /admin/x = "public"                     // left from before the admin area
fn g(r) = reply 401 "no"
routes /admin | g { route GET /x = "secret" }     // never reached: /admin/x is the public one
route GET /y = 1
route GET /y = 2                                  // never reached
route HEAD /z = 1                                 // never matches (the guide says so)
routes /_quillo { route GET /stats = 1 }          // never called (the guide says so)
```

All of these pass `quillo check` with no warning. The first is the dangerous one. When a guarded group is added to a program that already has a route for the same path, the older route keeps answering, without the middleware. The guide documents the `HEAD` and `/_quillo/` cases, but only `check` can catch them in code.

**Suggestion:** a `check` warning for a route whose method and path are fully covered by an earlier route with no patterns. Name the earlier route's line, and say so when the later one is in a group with middleware and the earlier one is not. Make `route HEAD` and a path under `/_quillo/` errors, with the guide's own wording.

### Q-542. Typed parameters take `NaN`, `inf` and `1e999` as floats (sent as JSON `null`), `+5`, `05` and `%205` as ints; `:x(str)` trims and `:x` does not

| Kind | Detected by | Attempts |
|---|---|---|
| silent | values given back as text and as JSON | 40 values |

```quillo
route GET /n/:id(int) = `int ${id}`
route GET /fn/:x(float) = json.stringify({ x, y: x * 2 })
route GET /f ? { x: float } = json.stringify({ x })
route GET /s/:x(str) = json.stringify({ x })
route GET /u/:x = json.stringify({ x })
```

| Request | Answer |
|---|---|
| `/fn/NaN`, `/fn/inf`, `/fn/1e999` | `{"x":null,"y":null}` |
| `/f?x=NaN` | `{"x":null}`, and `${x}` gives `NaN` |
| `/n/+5`, `/n/05`, `/n/%205` (a space) | `int 5` |
| `/s/%20a%20` | `{"x":"a"}` |
| `/u/%20a%20` | `{"x":" a "}` |
| `/s/%20` | `{"x":""}` (while `/s/` is a 404) |

`coerce` trims, then uses Rust's `parse` ([interp.rs:475-479](../../src/interp.rs#L475-L479)), which reads `NaN`, `inf`, `infinity` and `+`. A float parameter is used for prices, coordinates and amounts. A check such as `x <= 0.0 ? ... : ...` on `NaN` is a 500, logged as `cannot compare float with float`, which points nowhere near the URL. Arithmetic carries it (`x * 2`), and JSON writes it as `null`. `inf` passes `x > 0.0` and is stored as a number. One resource also gets many addresses (`/n/5`, `/n/05`, `/n/+5`), which caches and access logs count as different. A `str` parameter is trimmed, unlike the untyped one and unlike a `str` in a query or form pattern (not trimmed: `?q=%20x%20` gives `" x "`).

**Suggestion:** `float`, `num` and `dec` take only finite numbers written in decimal. `int` takes an optional `-`, then digits, with no leading `0` (`0` is taken, `05` is not). Nothing is trimmed, so `str` keeps what was sent, as the untyped form does. The same `coerce` serves query and form values, so this changes them too, as it should.

### Q-543. Form and query values meet patterns unevenly: a blank field with a default is a 400, `tag[]=a` gives `[]` silently, `bool` takes `yes` but not `TRUE`

| Kind | Detected by | Attempts |
|---|---|---|
| loud, silent | answers to 30 query strings and form bodies | |

```quillo
route GET /p ? { page: int = 1 } = `page=${page}`
route GET /b ? { on: bool } = `on=${on}`
route GET /l ? { id: [int] } = `ids=${json.stringify(id)}`
route POST /form { form: { agree: bool } } = `agree=${agree}`
route POST /tags { form: { tag: [str] } } = json.stringify(tag)
```

| Sent | Answer | Expected |
|---|---|---|
| `?page=` (a search form's blank field) | 400 | `page=1`: a blank field is a field left out |
| `?page=%20` | 400 | the same |
| `?on=yes`, `?on=on`, `?on=1` | `on=true` | |
| `?on=TRUE`, `?on=True` | 400 | `true`, or `yes` not taken either |
| `?on` (no `=`) | 400 | `true`, as HTML's boolean attributes read |
| form `agree` unchecked (not sent), no default | 400 | a hint that an unchecked box sends nothing, so it needs `= false` |
| `tag[]=a&tag[]=b`, as PHP- and Rails-shaped forms name a list | `[]`, 200 | the two values, or a warning |
| `?id=1,2` | 400 | fine as is |

The 400s are loud but say nothing (Q-544). The `tag[]` case is silent: the guide does say "`a[b]` is a key named so", but a list pattern given a form whose keys are `tag[]` gets an empty list and goes on, and the route saves no tags. A form built by a component library or copied from another framework writes `name="tag[]"` without anyone choosing it.

**Suggestion:** read an empty value as not sent, for a typed field that has a default (and for `T?`). For a `[T]` pattern, take the `name[]` keys when no `name` key is there. Read `bool` case-insensitively, or document the exact list. In the guide, under Query pattern, add a line on checkboxes: `{ form: { agree: bool = false } }`.

### Q-544. Every pattern miss is `bad request: no route pattern matches this request`, under `--watch` too, and nothing is logged

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the answer and the log under `--watch` | |

```quillo
route GET /p ? { page: int = 1 } = `page ${page}`
route POST /form { form: { agree: bool, name: str } } = "ok"
```

```
$ quillo run main.qwe --watch
$ curl -s 'localhost:4724/p?page=x'
bad request: no route pattern matches this request
$ curl -s -d 'name=x' localhost:4724/form
bad request: no route pattern matches this request
# the log:
GET /p?page=x -> 400 (0.0ms)
POST /form -> 400 (0.0ms)
```

Neither says which route was tried, which field missed, or what it wanted. On a form with ten fields, the developer must guess. In Q-543's cases (a blank field, an unchecked box), they cannot tell that the form, not the route, is wrong. The answer is fixed text ([server.rs:658](../../src/server.rs#L658)).

**Suggestion:** under `--watch`, answer and log the route that came closest and its first miss: `POST /form (main.qwe:2): form.agree is missing (an unchecked checkbox sends nothing; give it a default: agree: bool = false)`. In production, keep the plain text but log the same line, since a 400 nobody can explain is a support ticket. A JSON client (one that sent `accept: application/json`) could get `{"error": "...", "field": "agree"}`.

### Q-545. A host pattern compares the header as sent: `API.example.com`, `api.example.com:443` and `api.example.com.` miss

| Kind | Detected by | Attempts |
|---|---|---|
| loud | 5 `Host` headers | |

```quillo
routes /host { headers: { host: "api.example.com" } } {
    route GET /me = "me"
}
```

| `Host` sent | Answer |
|---|---|
| `api.example.com` | 200 |
| `API.example.com` | 400 |
| `api.example.com:443` | 400 |
| `api.example.com.` | 400 |

The guide gives this as the way to match a host ("Laravel's `domain()`"). A host is case-insensitive, a client may send the default port, and a trailing dot names the same host. Each one is a 400 that says nothing (Q-544). Where the guide's pattern is used to pick between two versions of a page (`/beta` with a header, `/beta` without), the same request falls through to the other one, which is silent.

**Suggestion:** give the request a normalized host (lower case, no default port, no trailing dot) as `request.host`, and have the guide match `{ host: "api.example.com" }` against that instead of `headers.host`. Or normalize `headers.host` itself before patterns see it.

### Q-546. Mistakes on the route line get other constructs' messages, or none

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` on 45 one-line programs | |

| Written | Message | What it could say |
|---|---|---|
| `route GET /health { ok: true }` (JSON meant) | expected end of statement, found `:`: a map's key is a name or text, and for one in a variable use `set(m, k, v)` | a route's `{` is its body; for a map write `= { ok: true }` |
| `route POST /x { json: j }` (no body) | the same map-key message | this request pattern has no body after it: `= ...` or `{ ... }` |
| `route POST /x with db { json: j } = j` | the same map-key message | the request pattern goes before `with` |
| `route POST /x { json: j } ? { q: str } = j` | the same map-key message | the query pattern (`? { }`) goes before the request pattern |
| `routes GET /a { ... }` | `routes` is no keyword: did you mean `route`? | a group takes no method: `routes /a { route GET ... }` |
| `routes admin { ... }` | `routes` is no keyword: did you mean `route`? | a group's path starts with `/` |
| `routes /admin with db \| g { ... }` | expected `{` and the group's routes | middleware goes before `with` |
| `routes /x ? { q: str } { ... }` | expected `{` and the group's routes | a group takes no query pattern; put it on each route |
| `fn role(r, name) = ...` then `\| role("admin")` | missing argument `name` in call to `role` | middleware is called with the request: `role("admin")` must give a function, `fn role(name) = (r) => ...` |
| `route GET /x \| g = 1` | expected `=` or `{` to start the route body | a route has no middleware of its own: put it in a group, or `request \| g \| ...` in the body |
| `/a/:id(int)x`, `/a:b`, `/a+b`, `/a /b` | expected `=` or `{` to start the route body | name the character the path cannot hold |
| `route GET /:if = 1`, `/:1`, `/f/*for` | none: accepted | a parameter named `if` or `1` can never be read |
| `routes /a { route GET /x = 1` then `routes /b { }` `}` | none: accepted | as at the top level: this group has no routes in it |

The `{ ok: true }` line is the one people will write most. Every other route answers JSON with a map, and `{` after the path also starts a request pattern, so the map-key hint points the wrong way. The nested empty group is accepted while a top-level one is an error ([parser.rs:858-860](../../src/parser.rs#L858-L860) checks only the outer group).

**Suggestion:** the messages in the right-hand column. The parser knows each case at the point where it fails: `parse_route_rest` after a pattern, `parse_group` at its `{`, `parse_path` at the character it stopped on.

## Smaller notes

- **Middleware factories work and are not documented.** `fn member_of(team) = (r) => team == 7 ? r : reply 403 ...` with `routes /t/:team(int) | member_of(team) { ... }` refuses team 8 and lets 7 through. The middleware expression runs in the route's scope, so the group's own parameters are there, already typed. That is the way to write role checks (`| role("admin")`), and the guide should show it, next to Q-546's message.
- **No way to take one route out of a group's middleware.** A CORS preflight carries no credentials, so `route OPTIONS /x` inside `routes /api | token` gets the middleware's 401, and the browser never sends the real request. Today the route must move out of the group. The guide's "Nothing answers CORS preflights by itself" could add this.
- **Narrowing a route narrows its group's middleware too.** In `routes /n | signed_in with time, db { route GET /status with db = ... }`, a `signed_in` that reads the clock is an error ("`signed_in` needs the time effect, but `route GET /n/status` is declared `with db`"). This follows from the guide's rule, and the message is right. But the guide's example of narrowing (`route GET /status with time` under `| signed_in with db, time`) works only while `signed_in` needs no database, and a real `signed_in` usually does.
- **`quillo docs` does not show a group's middleware or request pattern.** OpenAPI's `security` and a header `parameter` would say which routes are guarded. As it is, Q-536's case and a guarded group look the same in the output.
- **Route matching is a linear scan.** With 5,000 routes in one group, one `wrk` connection: the first route 47,000 requests a second, the last 18,300, an unknown path 27,600. About 7 ns a route. That is no problem at a few hundred routes. It is noted for the faster-than-Python rule. No Python framework was measured here.

## Questions for you

1. Q-534: should middleware fail closed (only the request or `null` goes on), even though some programs may now give `true` and work by accident? The repo's tests use `null`.
2. Q-539: should a group's middleware run for everything under its prefix (Express and nginx), or stay after the route matches (Laravel)? This decides whether the `ANY /*rest` line belongs in the guide or the fix belongs in the server.
3. Q-538: are server fns inside a group (`routes /admin | admins { server fn ban(...) }`) wanted? Or is a `check` warning and a line in the guide enough?

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `curl ... localhost:4718/admin/users?page=1` unquoted in zsh | a quick probe | zsh read `?` as a glob: "no matches found", and the rest of the batch did not run | 1 run |
| `? { q: str? }` | an optional query value, as a type is written | `` expected `,` or `}` in record pattern, found `?` ``; a pattern takes `q: str = null` | 1 run |
| `type(g)` to show what `g` was bound to | other languages' `type` | unknown name `type` (did you mean `take`?); used `json.stringify(g.headers.host)` | 1 run |
| `'routes /a { route GET /x = 1\n routes /b { } }'` in single quotes | a two-line probe | the `\n` stayed two characters: "unexpected character `\`"; rerun with `$'...'` | 1 run |
| a batch that stopped at the zsh error above, then the same program started again on the same port | rerun the batch | the first server was still running (its `stop.sh` never ran), and the second could not bind, so the second batch's middleware counter included the first's requests; found by `lsof` on the probe ports, and stopped by its PID | 2 runs, and nearly a wrong count in Q-539 |
| `x > 0` to show `NaN` passing a check | the usual comparison | `cannot compare float with int`; with `0.0`, `NaN` is a 500 too, not a pass, which changed Q-542's text | 2 runs |
| reading Q-536's 200 as the guard being skipped | the request had no `host: a` and still passed | it was an or-pattern; found by `quillo docs` showing no pattern, then a `match` with `{ a: 1 } \| g` | 3 runs |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| a group inside a function or a component; a route inside `if` | refused, with the right message |
| `/users/{id}`, `/{id:int}` | "path parameters are written `:id`" |
| a parameter twice, across a group and its route (`/t/:id` and `/:id`) | "path parameter `id` appears twice" |
| a path parameter named like the middleware (`/:guard` with `\| guard`) | `check`: "cannot call `guard`: it is str" |
| middleware that does not exist, is a map, or takes 0 or 2 arguments | each an error at the `\|` line |
| a group asking for more effects than the one around it, a route asking for more than its group | errors naming both `with`s |
| `routes / 2`, `let routes = [1]; len(routes)` | a division and a name, as the guide says |
| unparenthesized lambda middleware `\| (r) => ... ? r : reply 401 "no" { ... }` | parses; refuses without the header, lets it through with it |
| middleware whose database lookup fails (`db.one` on a missing table) in `x != null ? reply 403 : r` and `== null ? r : reply 403` | 403 both ways: an error value is not null |
| `/up/:name = file(\`uploads/${name}\`)` with `%2e%2e%2fmain.qwe`, `..%2fmain.qwe`, `a.txt%00` | 404 each |
| `:id(int)` with `1_000`, `0x10`, `1e3`, a 20-digit number | 404; `i64::MAX` and `i64::MIN` taken |
| `:d(date)` with `2026-02-29`, `2026-1-5`, `0000-01-01`, a time after the date | 404; `2024-02-29` taken |
| `?page=x&page=2` and `?page=2&page=x` | the last value counts, as documented |
| `{ form: { password: str } }` and `{ json: { password: str } }` with spaces around | kept as sent |
| `HEAD` on a guarded route | the middleware runs, 401 |
| `request.params` in a group route | the group's typed `team: 7` and the route's `id: "3"` together |

## Scripts

Each probe is a folder with a `main.qwe`, served by `run.sh` on its own port and stopped by its own PID (never `pkill`):

```bash
#!/bin/bash
# run.sh prog.qwe port: start quillo on the port, wait until it answers
Q=/Users/restiguay/dev/quillo-app1/target/release/quillo
dir=$(dirname "$1"); f=$(basename "$1")
cd "$dir"
"$Q" run "$f" --port "$2" > "$f.log" 2>&1 &
echo $! > "$f.pid"
for i in $(seq 1 50); do curl -s -o /dev/null "localhost:$2/_quillo/ready" && break; sleep 0.1; done
```

```bash
#!/bin/bash
# stop.sh prog.qwe...: stop what run.sh started
for p in "$@"; do kill "$(cat "$p.pid")" 2>/dev/null; done
```

The `check`-only probes (Q-546 and the table above) were one line each, written by a shell function: `t() { printf '%s\n' "$2" > "$1.qwe"; quillo check "$1.qwe"; }`. The 5,000-route program for the matching note was written by a Python one-liner: one group with a header pattern, `route GET /r{i}/:id(int) = id` for each `i`, then `route GET /first = 1`. It was measured with `wrk -t1 -c1 -d3s -H 'x-k: 1'`.
