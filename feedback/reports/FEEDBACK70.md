# Feedback from a review of how Quillo keeps forms from other sites' pages (CSRF): a `GET` that writes, and `Origin`'s scheme

| | |
|---|---|
| App | None: a review for the user's question of 2026-11-16 ("Please check how quillo secure form with CSRF by default"), then "Create FEEDBACK69.md" and "Create 70 then" |
| Quillo | The installed `quillo 0.8.5 (a57990a, 2026-11-16)`; the code read at the `v0.8.5` tag |
| Model | Claude (Opus 5.5, `claude-opus-5-5`), which ran the probes and wrote this report |
| Tested with | `quillo check` and `quillo run` on the small programs below, one curl request a case, the session cookie kept in a cookie jar, `--trust-proxy 127.0.0.1` with `X-Forwarded-Proto: https` for a request that came over HTTPS |

Numbers start at Q-965, after FEEDBACK69.md's Q-964 (agreed with the other session in this repo, which numbers from Q-967). Written at the user's word while main's commits since 0.8.5 are not yet released. Report only: no code changed.

What worked well: a form is safe from other sites by default, with nothing written for it. There is no token to put in each form, so no form forgets one, and nothing for each visitor is added to a page.

## What Quillo does

- **The gate** (src/server.rs, in `handle`, before any route is tried): a `POST`, `PUT`, `PATCH` or `DELETE` that the browser marks as coming from another site gets a `403`, "this site does not take form posts from other sites", unless `allow_cross_site` names its path. This applies to the bodies a page on another site can send without the browser asking first (`sends_form`): a form, multipart, `text/plain`, or none.
- **From another site** (`cross_site`, src/server.rs:200) means a `Sec-Fetch-Site` with any value but `same-origin` or `none`. So `same-site` (a sibling subdomain) and an unknown value are refused. A browser that sends no `Sec-Fetch-Site` is judged by `Origin`, which must name the request's `Host`; `Origin: null` is refused. A request with neither header, such as a webhook or a script, passes. It carries no visitor's cookies.
- **JSON** passes the gate. A page on another site cannot send it without a preflight, and Quillo's answer to the preflight has no CORS headers.
- **The session cookie** is `__Host-quillo_session=...; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax`, the same defaults `set_cookie` has.
- **The other doors that carry a visitor's cookies** call `cross_site` too:
  - `server fn` calls, which also take JSON only (`415` otherwise);
  - the markdown preview;
  - the channel ticket (`/_quillo/wt-ticket`);
  - the WebSocket and live connections (`/_quillo/wt`, `/_quillo/live`, and `--watch`'s `/_quillo/events`);
  - MCP's door (src/mcp.rs).
- **`allow_cross_site("/sso")`** opens `/sso` and the paths under it, not `/ssox` (src/site.rs, `takes_cross_site`).
- **Tests:** 43 lines of the tests name `Sec-Fetch-Site`, and 10 test files name it, `allow_cross_site` or the 403's text, `tests/security.rs` among them.

The probe program:

```quillo
allow_cross_site("/sso")

let bumps = 0

server fn save(text: str) -> str = text

route POST /notes { form: { text: str } } = "saved"
route PUT /notes { form: { text: str } } = "put"
route POST /api { json: { text: str } } = "json saved"
route POST /sso/callback = "sso ok"
route POST /ssox = "ssox"
route GET /login {
    set_session({ user: 1 })
    return "in"
}
route GET /bump {
    bumps = bumps + 1
    return `bumped ${bumps}`
}
```

| Request (a form body unless said) | Answer |
|---|---|
| `Sec-Fetch-Site: cross-site` | 403 |
| `Sec-Fetch-Site: same-site` (a subdomain) | 403 |
| `Sec-Fetch-Site: CROSS-SITE`, or an unknown value (`zzz`) | 403 |
| `Sec-Fetch-Site: same-origin`, or `none` (an address typed, a bookmark) | 200 |
| `Sec-Fetch-Site: same-origin` with `Origin: https://evil.example` | 200. A page cannot set `Sec-Fetch-Site`, so only a client with no visitor's cookies sends this |
| No `Sec-Fetch-Site`, `Origin` another host, or `null` | 403 |
| No `Sec-Fetch-Site`, `Origin` the request's host | 200 |
| No `Sec-Fetch-Site`, `Origin` the request's host with the other scheme | 200 (Q-966) |
| Neither header (a webhook) | 200 |
| From another site: multipart, `text/plain` (to a `json:` route too), `Text/Plain; charset=utf-8`, no body and no content type, a `PUT` | 403 |
| From another site: `application/json` | The gate lets it through (200). Its preflight gets `204` with `allow: OPTIONS, POST` and no `access-control-*`, so a browser never sends it |
| `POST /sso/callback` from another site, `/sso` allowed | 200 |
| `POST /ssox` from another site | 403 |
| A `server fn` call from another site (JSON) | 403 |
| A `server fn` call from the program's page, as `text/plain` | 415 |
| A WebSocket upgrade of `/_quillo/wt` with `Origin: https://evil.example`; `/_quillo/live` from another site | 403 |
| A `GET` that writes, from another site | 200 (Q-965) |

## New issues

### Q-965. A `GET` route that writes runs from a link on another site, with the visitor's session, and `quillo check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent (the write happens, and nothing warns of it before or after) | this review: a curl probe on 0.8.5, then `quillo check` | 1 |

```quillo
db.open(":memory:")
db.exec("create table posts (id integer primary key, owner int not null)")

route GET /login {
    set_session({ user: 1 })
    db.exec("insert into posts (owner) values (1), (1), (1)")
    return "in"
}
route GET /count = `${db.query("select count(*) as n from posts")[0].n}`
route GET /posts/delete-all {
    let u = session().user ?? return respond(401, "log in first")
    db.exec("delete from posts where owner = ?", u)
    return "deleted"
}
```

On 0.8.5, signed in through `/login`, with the cookie kept:

```
quillo check q965.qwe                                    q965.qwe: ok (4 routes, 0 functions, 0 components)
GET /posts/delete-all, Sec-Purpose: prefetch             503, 3 rows left (the prefetch guard)
GET /posts/delete-all, Sec-Fetch-Site: cross-site,
    Sec-Fetch-Mode: navigate, the session cookie         200 "deleted", 0 rows left
```

The gate covers the methods that write by convention, not `GET`. `SameSite=Lax` sends the cookie on a top-level navigation: a link followed, `location = ...`, or a `<form method="get">`. So a page on another site, or a link in an email or a chat, that sends the visitor to `/posts/delete-all` runs it as them.

Quillo already knows this route writes: the same request as a prefetch is stopped (`PREFETCH_REFUSED`). But nothing says so to the developer. The guide does not mention it either. Its "Prefetching changes nothing" ends with "So a `GET` that does something (a logout link, a counter) is never done by pointing at it", which reads as if such a `GET` were fine.

Not every `GET` that writes is wrong:
- qwertime's `GET /live` notes the member as present and their device seen (its main.qwe:946-957), on purpose, as the stream opens.
- An email's "confirm your address" link writes, and is followed from another site by design.
- A logout link is a write that few sites protect.

Refusing such writes at runtime would break these cases, so a warning that names them is the better fix.

Expected: `quillo check` warns about a `GET` route that writes, points to `POST`, and offers a way to say the write is on purpose (see the questions below). The guide says why under Routes.

| Impact | |
|---|---|
| **Functionality** | None at runtime. A warning in `check` would name `GET` routes that write in existing programs (qwertime's `/live` at least), until each is made a `POST` or marked as on purpose |
| **User** | A visitor who follows a link from another site, an email or a chat runs the write as themselves (data deleted, a setting changed, a follow or a vote cast) without seeing it coming |
| **Requests** | One `GET`. A fix in `check` adds none |
| **Security** | CSRF through `GET`, the way that is left once posts from other sites are refused: any write a `GET` route does with the session |

Proposed fix: check.rs's effects mask (`fn_effects`, check.rs:5162) has one `db` bit for reads and writes alike (src/ast.rs, `EFFECTS`). The warning needs the list the prefetch guard stops at runtime (`PREFETCH_REFUSED`, in src/interp.rs, src/db.rs and src/server.rs):
- `db.exec`, and SQL whose words write;
- an assignment to a global;
- `set_session` and `set_cookie`;
- files;
- `publish`, `http.post`, and calls to services.

It should warn only on what the program's text makes certain: SQL written in the program, not SQL built at runtime. Add a guide line under Routes ("A `GET` must not change anything a visitor would not want changed for them: another site can send them to it") and change the line in "Prefetching changes nothing" to match.

### Q-966. With only `Origin` sent, its scheme is not compared: `http://host` passes for a request that came over HTTPS

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading `cross_site` (src/server.rs:200), then a probe | 1 |

`cross_site` keeps what follows `://` in `Origin` and compares it with `Host`. With `quillo run q965.qwe --trust-proxy 127.0.0.1` (the program above, with `route POST /notes { form: { text: str } } = "saved"`), each request is a form post to `/notes` with `X-Forwarded-Proto: https` and no `Sec-Fetch-Site`:

| `Origin` | Answer |
|---|---|
| `http://127.0.0.1:4793` (another origin: the other scheme) | 200 |
| `https://127.0.0.1:4793` | 200 |
| `https://evil.example` | 403 |
| `http://127.0.0.1:4793`, with `Sec-Fetch-Site: cross-site` | 403 |

Two conditions are both needed to use it:
- the attacker can serve the visitor a page at `http://` on the same host, from the visitor's network (public Wi-Fi, a hostile router);
- the visitor's browser sends no `Sec-Fetch-Site`: Chrome before 76, Firefox before 90, Safari before 16.4.

A browser that old may also count the http and https addresses of one host as one site for cookies, so `SameSite=Lax` may not stop the post either. Neither Quillo nor quillolang.org sends `Strict-Transport-Security` (checked 2026-11-16, an HTTPS answer from each), so the browser does load `http://` when sent there. HSTS would close this case, and others.

Expected: when the request's scheme is known (TLS, or `x-forwarded-proto` from a trusted proxy, as `came_over_https` in src/session.rs reads it), an `Origin` with the other scheme is another site.

| Impact | |
|---|---|
| **Functionality** | None for the program's own pages: their `Origin` has the request's scheme. A site whose `http://` pages post to its `https://` address would get 403s, from old browsers only. This setup is unusual, because a proxy usually redirects `http://` |
| **User** | None in a current browser |
| **Requests** | None |
| **Security** | Closes a narrow case that needs an old browser and an attacker on the visitor's network |

Proposed fix: in `cross_site`, compare `Origin`'s scheme with the request's when the server knows the request's scheme. When it doesn't (no TLS, no trusted proxy header), compare the hosts as now. Under `--watch` on `localhost` both are `http`, so nothing changes there.

### Q-986. `came_over_https` believes any client's `X-Forwarded-Proto`, where `cross_site` now believes only a `--trust-proxy` proxy's

| Kind | Detected by | Attempts |
|---|---|---|
| silent (two readings of one header that disagree) | a review of Q-966's fix (branch security, 2026-11-17), reading src/session.rs:423 | 1 |

`came_over_https` (src/session.rs) takes the last `X-Forwarded-Proto` from anyone. It decides two things:
- the default `Secure` flag of `delete_cookie`'s clearing cookie;
- the scheme of the origin kept with a push subscription (src/push.rs `origin_of`).

Since Q-966, `cross_site` reads the same header only from a `--trust-proxy` proxy. A client that sends the header itself changes only its own answer and its own subscription: the origin's host is the client's `Host` anyway, so the header adds no reach.

Expected: the two agree. The open part is what to do for a site behind a proxy it never named with `--trust-proxy`. Requiring it would make those sites' cookie deletions non-`Secure` and their push origins `http://`, with nothing said, and the guide tells people to send the header but never names `--trust-proxy` for it.

| Impact | |
|---|---|
| **Functionality** | None today. Requiring `--trust-proxy` changes two defaults for sites behind an unnamed proxy |
| **User** | None: a client only changes what it is itself sent |
| **Requests** | None |
| **Security** | Consistency only: no other visitor is reached |

Proposed fix: require `--trust-proxy` (as `cross_site` does), and print one line at startup when requests carry `X-Forwarded-Proto` from a peer that isn't named: "behind a proxy? `--trust-proxy <its address>`".

### Q-987. Under `quillo fcgi`, an `Origin`'s scheme is never compared (Q-966)

| Kind | Detected by | Attempts |
|---|---|---|
| silent (a fix that does not reach one way of serving) | the fix of Q-966 (branch security, 2026-11-17), reading src/fcgi.rs | 1 |

FastCGI requests reach the HTTP parser over a socket pair whose peer is the browser's `REMOTE_ADDR`, so they never carry the trusted-proxy mark, and the web server's `HTTPS` parameter is not read. Q-966's scheme comparison therefore never applies there, and the browser's own `HTTP_X_FORWARDED_PROTO` is passed through as it came.

Expected: under `quillo fcgi` the web server is the proxy. Its `HTTPS` parameter (`on`, or absent) is the request's scheme, and a client's `X-Forwarded-Proto` is not believed.

| Impact | |
|---|---|
| **Functionality** | None for current browsers, which send `Sec-Fetch-Site` |
| **User** | None in a current browser |
| **Requests** | None |
| **Security** | Q-966's narrow case (an old browser, and an attacker on the visitor's network) stays open on shared hosting |

Proposed fix: pass a "scheme known" flag from fcgi.rs through `Hand::give` into the parser, set from `HTTPS`, and drop a client's `X-Forwarded-Proto` there.

## Questions for the user

1. Q-965: how does a `GET` route say it writes on purpose, so `check` does not warn (qwertime's `/live`, a confirm link)? Or does `check` warn only about some writes (SQL that deletes or updates rows the session owns)?
2. HSTS: should Quillo send `Strict-Transport-Security` by default on answers that came over HTTPS, or leave it to the proxy and `site_headers`? quillolang.org sends none today.
