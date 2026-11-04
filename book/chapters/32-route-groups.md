# Route groups

Routes that share a start of their path, a check before they run, or a request pattern
are written once for all of them, in a `routes` group. It is Laravel's
`Route::prefix()->middleware()->group()` as syntax: a prefix, middleware after `|`,
an optional pattern, then the routes in braces. Everything in this chapter is one program;
put the parts in one file to try it.

## A prefix

The group's path goes before each route's own. `/` inside the group is the prefix
itself, and the group's parameters are the routes' to use:

```quillo
routes /admin {
    route GET / = <AdminHome/>                      // GET /admin
    route GET /users = list_users()                 // GET /admin/users
    route POST /users/:id(int)/ban { ban(id) }      // POST /admin/users/7/ban
}

routes /teams/:team(int) {
    route GET / = team_page(team)                   // GET /teams/7
    route GET /members = members(team)              // GET /teams/7/members
}
```

A typed parameter in the prefix (`:team(int)`) is matched and converted as in a route
line, and a value that does not fit lets matching go on to the next route, so
`/teams/abc` can still be answered by a later `route GET /teams/:slug`.

## Middleware

A check is a function that takes the request and either gives it back or ends the
request. In a group it goes after the prefix, one `|` per check, and runs before the
route's body:

```quillo
fn signed_in(r) = session().user != null ? r : reply 303 redirect(`/login?next=${url_encode(r.path)}`)
fn admins(r) = is_admin(session().user) ? r : respond(403, "admins only")
fn logged(r) { print(r.method, r.path); r }

routes /admin | logged | signed_in | admins {
    route GET / = <AdminHome me={me()}/>
    route GET /users = list_users()
}
```

What a middleware gives back decides what happens:

- **A `reply`** answers the request there, as it does anywhere: `reply 303 redirect(...)`
  sends the visitor to the login page, `reply 401 { error: "..." }` answers JSON.
- **A response value**, from `respond(...)` or `redirect(...)` returned rather than
  replied, answers the request too.
- **The request, a copy of it (`merge(r, ...)`), or null,** lets the route run. Giving
  the request back is the convention, so the same function works as a pipeline stage
  inside a route (`request | signed_in | ...`) and as a group's middleware. Null goes on
  too, so `quillo check` warns about a middleware that gives the request back in one
  place and can end with no value in another (an `if` with no `else` after a `return
  r`): whoever reaches that end is let in.
- **Anything else** stops it *(since 0.7: these let the route run)*: `false` answers
  403, an error value answers as a route's error does, and any other value (`true`,
  text, a map other than the request, `{ error: "bad api key" }`) is a 500 that names
  the middleware. So a check is written `(r) => ok(r) ? r :
  reply 403 "..."`, not as a predicate, and `quillo check` says so when a middleware can
  only be a `bool` or text.
- What it gives does not reach the route: a route that needs the user reads it again
  (`session()`, `me()`), which is cheap.

Order: outer groups' middleware first, then the inner's, each in the order written.
It is given the request as sent, whatever the route's patterns bind.

It runs before the server's own answers too *(since 0.7)*: a request under the group's
path that no route of it takes (another path, method or parameter type, a pattern it does
not match, an `OPTIONS`) goes through the group's middleware first, so only those it lets
in learn what is there from a 404, a 400, a 405 and its `Allow`.
`quillo check` checks each call at the line the middleware is written on, as it checks
any other call.

## A group pattern

A request pattern after the middleware is matched as well as each route's own, and
binds the same way. It is how a group serves one host, Laravel's `domain()`:

```quillo
routes /api { headers: { host: "api.example.com" } } {
    route GET /me = me()
    route GET /users = list_users()
}
```

A request the pattern does not match goes on to the next route, so the same paths can
be declared again for another host below.

## Middleware without a prefix

A group at `/` adds nothing to the paths and is the way to put a check before a set of
routes, including a whole file's:

```quillo
routes / | signed_in {
    route GET /settings = <Settings me={me()}/>
    route POST /settings { save_settings(request.form); redirect("/settings") }
}
```

This is also the answer to "a hook before every route": there is no global hook, but a
`routes /` group around the routes gives them one, and routes outside it (the login
page, the health check) stay open.

## Nesting

Groups nest. The inner group adds its prefix and its middleware after the outer's:

```quillo
routes /api | api_key {
    route GET /private = { ok: true, from: request.ip }

    routes /items | json_only {                      // /api/items, api_key then json_only
        route POST / = add_item(request.json)
        route DELETE /:id(int) = remove_item(id)
    }
}
```

## What its routes may do

A group takes `with`, after its middleware, as a function does: what every route in it,
and its middleware, may use at most. A route in it may narrow it; one that asks for an
effect the group does not allow is an error, and so is an inner group that does:

```quillo
routes /reports | signed_in with db, time {
    route GET /daily = daily_totals()                    // db and time
    route GET /now with time = str(now())                // time only
}
```

`quillo check` says it before the program runs, and the program refuses it as it runs:
`` `http.post` needs the http effect, but `route GET /reports/daily` is declared `with db,
time` ``. A route outside any group takes `with` the same way: `route GET /day/:d with db
= ...` (see [Effects](#effects)).

## Rules

- A group holds only `route`s and `routes` groups, at the top level of a file. Its
  routes keep their place among the file's others: order decides which route matches
  first, as always.
- Middleware goes on a group, never on a single route line: `route POST /items |
  json_only = ...` is a syntax error. For one route, nest a group of one, or write the
  check as a pipeline stage in its body.
- `routes` is a keyword only at the start of a group. Elsewhere it is a name like any
  other (`routes / 2` divides).
- Routes from groups are routes: `quillo docs` lists them by their whole path
  (`/admin/users`), and `quillo check` checks them as it checks any route.

## The example, run

A small program with an API behind a key, JSON required for writes, and an admin area
behind a session:

```quillo
let items = []

fn api_key(r) = r.headers.authorization == `Bearer ${env("API_TOKEN")}` ? r : reply 401 { error: "unauthorized" }
fn json_only(r) = starts_with(r.headers["content-type"] ?? "", "application/json") ? r : reply 415 { error: "send JSON" }
fn signed_in(r) = session().user != null ? r : reply 303 redirect(`/login?next=${url_encode(r.path)}`)

routes /api | api_key {
    route GET /items = items
    routes /items | json_only {
        route POST / { items = push(items, request.json); { count: len(items) } }
    }
}

routes /admin | signed_in {
    route GET / = <p>Admin, {session().user}</p>
}

route GET /login = <form method="post"><input name="user"/><button>Sign in</button></form>
route POST /login { form: { user: str } } { set_session({ user }); redirect("/admin") }
```

```sh
curl -i localhost:8080/api/items                              # 401 unauthorized
curl -H "Authorization: Bearer $API_TOKEN" localhost:8080/api/items
curl -H "Authorization: Bearer $API_TOKEN" -d a=1 localhost:8080/api/items      # 415 send JSON
curl -H "Authorization: Bearer $API_TOKEN" --json '{"name":"mug"}' localhost:8080/api/items
curl -i localhost:8080/admin                                  # 303 to /login?next=%2Fadmin
```
