# Feedback from reviewing how services authenticate, and what industry-standard authentication between them would need

| | |
|---|---|
| App | None. The question: can a Quillo service be made with the authentication other systems use between services (OAuth 2.0 client credentials and JWT access tokens, mutual TLS, API keys, signed requests), not only Quillo's shared secret? Part 1 reviews what a call between services proves today and to whom. Part 2 lists what a program can do now, and how. Part 3 is a design for `auth` on `service` |
| Quillo | 0.6.0 (23146e4, 2026-11-02). The binary is `feedback27-35-fixes`'s release build, built 11:57 today with that branch's uncommitted changes and copied to a scratch folder so a rebuild could not change it mid-run. No feature for other authentication exists on that branch or on main: nothing in the diff to [src/service.rs](../../src/service.rs) touches it. Line numbers are the branch's working tree; [src/fetch.rs](../../src/fetch.rs) is unchanged from 23146e4 |
| Model | Claude (Opus 5.5) |
| Tested with | Two services in one folder (`billing.qwe` with `server fn refund` and `server fn whoami`, an app calling it), run apart with `QUILLO_SERVICE_BILLING`. A 15-line Python client that signs calls as any service holding the secret could. A Python server standing in for an impostor copy of `billing`. A routes-only service guarded by a bearer JWT through `jwt.qwer`, from ~/dev/quillo-website/plugins. Read: [src/service.rs](../../src/service.rs) (`secret`, `join_here`, `signature`, `answer`, `run_call`, `set_opts`, `remote`), [src/fetch.rs](../../src/fetch.rs), [src/crypto.rs](../../src/crypto.rs), the plugins `jwt.qwer`, `oidc.qwer` and `http_signatures.qwer`, and USERGUIDE.md (Services, Copies that join, Routes) |

No Quillo source was changed. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched, as another session is working in the checkout. Each repro is in its own section, and the probe scripts are below. The user first named this report 37, but FEEDBACK37.md already holds another session's report (Q-452 to Q-457); the user then said 38. Numbers start at Q-458.

What worked well: what Quillo's own signature covers, it covers well. A call is HMAC-SHA256 over the time, the service, the function, the tenant, the call id and the body. The comparison is constant-time, and a call more than five minutes old is refused. A call signed for `orders` and sent to `billing` is refused with both names. A call sent twice, by a retry or a replay, gets the first answer back, and the function runs once. A secret of 3 bytes is warned about at start. `/_quillo/ready` names services but not their addresses, and `/_quillo/describe` is 404 unless `--describe` is given. For calling the outside world, `http.*` takes any header, so a bearer token or an API key goes out with one line. The plugins already cover much of the standard ground: `jwt.qwer` (HS256 and RS256), `oidc.qwer` (discovery, PKCE, ID tokens against the provider's JWKS with RS256 and ES256), `paseto`, `http_signatures`, `aws_sign`, `gcs_sign`, `ldap`, `saml`, `passkeys` and `totp`.

Fix Q-458 and Q-459 first. Every service holds the one key that lets it be any other: it can call any function of any service, in any tenant, and it can join as any service and take that service's calls, arguments included. Then Q-460, which is the user's ask itself: a server fn answering another service sees nothing of the request but its id, and a caller can add nothing to a call, so no standard scheme can even be layered on in code.

## The answer

### What a call between services proves today

| Question | Today | Issue |
|---|---|---|
| Is the caller one of ours? | yes: it knows `QUILLO_SERVICE_SECRET` | |
| Which service is calling? | no: every service has the same key; `x-quillo-caller` is sent but not signed, and nothing reads it | Q-458 |
| May this caller call this function? | no way to say: every function is open to every key holder | Q-458 |
| In which tenant? | the tenant is signed, but any key holder can name any tenant | Q-458 |
| On behalf of which user? | not carried, and a server fn cannot read a header to find out | Q-460 |
| Is the callee who it says it is? | no: a joined copy is anyone with the key; the answer is not signed, and a copy is called over `http://` | Q-459 |
| Can the key change without downtime? | no: one value, no key id, no previous key | Q-461 |
| Can it ride on mTLS? | only through a sidecar: Quillo cannot present a client certificate or trust a CA of its own | Q-462 |
| Can a gateway or a service in another language call a `server fn` with a standard token? | no: it must reimplement Quillo's HMAC format | Q-460 |

### What a program can do now, and how

| Mechanism | Between Quillo services, as `service` calls | As routes (a JSON API) called with `http.*` |
|---|---|---|
| Quillo's shared HMAC secret | yes, built in | n/a |
| Bearer JWT, HS256 | no (Q-460) | yes with `jwt.qwer`, but only for a token with no `aud` (Q-463) |
| Bearer JWT, RS256 / ES256 from an identity provider | no | RS256 with a PEM, by hand; JWKS only for ID tokens in `oidc.qwer` (Q-463) |
| OAuth 2.0 client credentials | no | by hand: `http.post` to the token endpoint and keep the token until it expires; no plugin |
| API key | no | yes: compare the header with `crypto.equal` |
| mTLS | only a sidecar that does both ends (Envoy, Linkerd) | the same; the client cannot present a certificate (Q-462) |
| Signed requests | Quillo's own format | `http_signatures.qwer` (draft-cavage, `rsa-sha256` only, not RFC 9421); `aws_sign.qwer` to call AWS |
| The visitor's identity | as an argument the callee must trust | the visitor's token forwarded in a header |

So a service can use industry-standard authentication today only by **not being a Quillo service**: by being routes behind a bearer check, called with `http.*`. That works (checked below), but it gives up what makes `service` worth having. Gone are `quillo check` of each call against the function's types, contracts (409 on a mismatch), retries with one call id run once, circuits, deadlines passed down the chain, `.later` with the transaction, published topics that wake the caller's pages, and the same code in one process or split.

The routes way, checked on this binary:

```quillo
// orders.qwe: the service, as routes
use "jwt.qwer"
let KEY = env("JWT_KEY") ?? ""
let auth = {
    service: (req) => {
        let claims = jwt_verify(replace(req.headers.authorization ?? "", "Bearer ", ""), KEY)
        if is_err(claims) { reply 401 { error: claims.error } }
        if claims.scope != "orders:refund" { reply 403 { error: "scope" } }
        claims
    },
}
route POST /api/refund {
    let who = auth.service(request)
    { refunded: request.json.cents, for: who.sub }
}

// caller.qwe
use "jwt.qwer"
let KEY = env("JWT_KEY") ?? ""
route GET /go {
    let token = jwt_sign({ sub: "service:shop", scope: "orders:refund" }, KEY, 60)?
    http.post("http://127.0.0.1:18631/api/refund", { cents: 500 }, { authorization: `Bearer ${token}` })?
}
```

`GET /go` gives `{"refunded":500,"for":"service:shop"}`, and a bad token gives 401 `InvalidToken`. With `aud: "orders"` in the claims, which a standard service token has, it gives 401 `InvalidAudience` (Q-463).

### The design asked for

One option, `auth`, where a service is declared, says how its calls are authenticated. One declaration in the called program says what it accepts, and who the caller is shows in `request.caller`. The wire, the retries and the contracts stay as they are; only the proof changes.

```quillo
// the caller: how it proves who it is to `billing`
service billing = "billing.qwe" { auth: "quillo" }                          // today's, the default
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY") } }           // its own key: signed as today, with kid=shop
service billing = "billing.qwe" { auth: { oauth: {                         // OAuth 2.0 client credentials (RFC 6749 §4.4)
    token_url: env("IDP_TOKEN_URL"), client_id: "shop", client_secret: env("SHOP_CLIENT_SECRET"),
    scope: "billing", audience: "billing",
} } }                                                                       // token kept until 30 s before `exp`, fetched again on a 401
service billing = "billing.qwe" { auth: { bearer: () => vault.token("billing") } }   // any token, made by the program
service billing = "billing.qwe" { auth: { mtls: { cert: "/etc/shop.crt", key: "/etc/shop.key", ca: "/etc/ca.pem" } } }
service billing = "billing.qwe" { forward: ["authorization"] }              // the visitor's token goes along (or RFC 8693 token exchange)
```

```quillo
// billing.qwe: what it accepts, and who may call what
accept_calls {
    keys: { shop: env("SHOP_KEY"), orders: env("ORDERS_KEY") },              // several values a name, while one is rotated
    jwt: { issuer: env("IDP"), audience: "billing", jwks: env("IDP") + "/.well-known/jwks.json" },
    client_cert: { header: "x-forwarded-client-cert" },                     // identity from the proxy that ended mTLS, under --trust-proxy
    check: (caller) => caller.name != "" ?? reply 401 { error: "who?" },     // anything else, in Quillo
}

server fn refund(account: int, cents: int) for ["shop"] {                   // only these callers
    // request.caller: { name: "shop", by: "jwt", scopes: ["billing"], claims: {...} }
    // request.user: the visitor's identity when the caller forwarded it and it was verified
}
```

| Mechanism | Caller's `auth` | Callee's `accept_calls` | `request.caller.by` | Standard |
|---|---|---|---|---|
| Shared secret | `"quillo"` | always on when `QUILLO_SERVICE_SECRET` is set; can be turned off | `"quillo"`, `name` from the signed `x-quillo-caller` | |
| A key for each service | `{ key }` | `keys` | `"key"` | HMAC with a key id, as AWS access keys; or RFC 9421 HTTP Message Signatures with `Content-Digest` (RFC 9530), so other languages can use a library |
| JWT access token | `{ oauth }`, `{ bearer }` | `jwt` | `"jwt"`, with `sub`, scopes, claims | RFC 6749 §4.4, RFC 9068, RFC 7517 (JWKS), RFC 8707 (audience) |
| mTLS | `{ mtls }` | `client_cert` (Quillo does not end TLS, so a proxy does, and its header is trusted only under `--trust-proxy`) | `"mtls"`, the certificate's subject or SPIFFE id | RFC 8446, RFC 8705 for tokens bound to certificates |
| API key | `{ bearer: () => env("KEY") }` | `keys` taking a bearer | `"key"` | |
| The visitor | `forward` | `jwt` checks it too | `request.user` | RFC 8693 token exchange |

Joining uses the same `auth`: a copy proves it is `billing` with `billing`'s own key, certificate or token, and the app takes it at the URL it gives (`--join-as https://...`), not always `http://`. `quillo check` warns about an `accept_calls` with nothing in it, a `for [...]` naming a service nobody declares, and `auth: { key }` or `{ oauth }` reading a secret from a literal. `/_quillo/describe` puts each function's `for` into OpenAPI's `security`, so a gateway can enforce it too.

Order of work: a key for each service first (Q-458, Q-459, Q-461 inside today's wire format: the header gains `kid=`, the callee keeps a table, the app takes a copy only with that service's key), then `request.caller` and `for [...]` (Q-458's authorization), then the `accept_calls` hook and caller headers (Q-460), then `oauth`, `jwt` and `mtls`, which bring dependencies (rustls client certificates, a JWKS cache).

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-071 | The secret is set on every machine by hand, as FEEDBACK10 said: it is also the one thing that makes a machine trusted by all the others (Q-458) | 0 |

## New issues

### Q-458. One secret makes every service every other: any service can call any function of any service, as any caller, in any tenant, and a server fn cannot tell who called it

| Kind | Detected by | Attempts |
|---|---|---|
| security, silent | signing a call by hand with the secret | 1 |

```sh
export QUILLO_SERVICE_SECRET=0123456789abcdef0123456789abcdef
quillo run billing.qwe --port 18601 --as-service       # server fn refund(account: int, cents: int) { { refunded: cents, request: request } }
# notify, or any service that has the secret, signs a call billing never expects from it:
python3 sign.py http://127.0.0.1:18601 billing refund '[7, 99999]' \
    '{"x-quillo-caller": "/notify", "authorization": "Bearer not-checked", "x-quillo-deadline": "3600000"}'
# 200 {"ok":{"refunded":99999,"account":7,"request":{"id":"1337be5c8b61fa0b","call":"7ade24463cdaed9c"}}}
```

The callee's `request` is `{ id, call }` and nothing more ([src/service.rs:621](../../src/service.rs), set at 1069), so `refund` cannot tell `shop` from `notify`. `x-quillo-caller` is set at [src/service.rs:840](../../src/service.rs) but is not in the signature ([src/service.rs:710](../../src/service.rs)) and is never read. Neither are `x-quillo-deadline`, `x-quillo-contract` and `x-request-id`. On a plain-HTTP network, someone who sees a call can change them without breaking it: shorten its deadline, make it a 409, or give the log another request id. The tenant is signed, but whoever signs picks it. So the most exposed service in a system (the one that sends email, or reads uploads) holds the key to the ledger, and a leak of its environment is a leak of all. The guide says the secret is "the same on every service" and to use HTTPS "so calls cannot be seen". It does not say that every service with the secret can do anything any other can.

**Suggestion:** a key for each service (the design's `auth: { key }`, `accept_calls { keys }`): the signature header gains `kid=<caller>`, the callee looks the key up, and `request.caller` is `{ name, by }`. Sign `x-quillo-caller` (which the key id makes the same thing), `x-quillo-deadline` and `x-quillo-contract` as well (`quillo-service-3`, with `-2` still taken while a system upgrades). Add `server fn f(...) for ["shop"]`, checked before the arguments are read. Until then, the guide should say in Services that the secret is all-or-nothing.

### Q-459. Anyone with the secret can join as any service: the app sends that service's calls, arguments and tenant included, to the joiner, takes its answers as the service's, and calls it over plain `http://`

| Kind | Detected by | Attempts |
|---|---|---|
| security, silent | a Python server joined as `billing` | 1 |

```sh
quillo run app.qwe --port 18610              # service billing = "billing.qwe", in the app's process
curl -s 127.0.0.1:18610/refund               # {"refunded":500,...}: billing's own answer
python3 fake.py 18612 &                       # answers every /_quillo/service/* with {"ok": {"refunded": 0, "by": "an impostor"}}
python3 join.py http://127.0.0.1:18610 billing 18612
# 200 {"service":"billing","at":"http://127.0.0.1:18612"}
curl -s 127.0.0.1:18610/refund               # {"refunded":0,"by":"an impostor"}
# fake.log: GOT /_quillo/service/refund billing None [7,500]
```

`join_here` ([src/service.rs:128](../../src/service.rs)) checks that the join was signed with the secret and that the program declares a service of that name. It never checks that the joiner is that service: `notify` can join as `billing`. A joined copy then takes every call, as the guide says ("the copies that joined take every call"), so from the next call on, billing's arguments go to the joiner and its answers are believed, since answers carry no signature. The URL is always made as `http://{ip}:{port}` ([src/service.rs:166](../../src/service.rs)), so a copy on another machine is called in the clear, whatever the guide says about HTTPS between services. `/_quillo/join` is on the app's public port, and nothing turns it off for an app that never wants copies.

**Suggestion:** with keys for each service (Q-458), take a copy of `billing` only when the join is signed with `billing`'s key. Let a copy give the URL it is called at (`--join-as https://billing-3.internal:8443`), checked against its connection's address or a list. Sign answers with the callee's key, or rely on mTLS (Q-462). Add `--no-join`, or join only for services named in `--allow-join billing,risk`.

### Q-460. A server fn answering another service sees none of the request, and a caller can add nothing to a call, so no standard scheme can be added in code: not a bearer token, not the visitor's identity, not a proxy's verified client certificate

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a call with extra headers; reading `set_opts` | 1 |

```sh
python3 sign.py http://127.0.0.1:18601 billing whoami '[]' '{"authorization": "Bearer eyJhbGciOi...", "x-user": "alice"}'
# 200 {"ok":{"id":"186821b28661417d","call":"c3604377d519b366"}}     server fn whoami() { request }
```

```quillo
service billing = "billing.qwe" { headers: { authorization: "Bearer ..." } }
// service billing: no option `headers` (timeout, retries, failures, cooldown)
```

A route's `request` has `headers`, `ip`, the session, and the rest. A `server fn` called by a service gets `{ id, call }` ([src/service.rs:621](../../src/service.rs)). The only options on `service` are `timeout`, `retries`, `failures` and `cooldown` ([src/service.rs:315](../../src/service.rs)). So:
- a service cannot accept an OAuth access token, an API key or an mTLS identity passed on by a proxy (`x-forwarded-client-cert`), even in its own code;
- the visitor's identity does not travel. Billing must take `user_id` as an argument and trust whoever sent it, which is Q-458 again;
- a gateway, or a service in Go or Java, can call a `server fn` only by reimplementing Quillo's HMAC format. The guide documents that format, but it is not a standard any library speaks.

**Suggestion:** the design's `accept_calls { check: (caller) => ... }`, run before the arguments are read, with the call's headers, and whatever it returns as `request.caller`. On the caller's side, `auth: { bearer: () => ... }` and `forward: [...]`. That much makes any scheme possible in Quillo or in a `.qwer`, before any built-in verifier is written.

### Q-461. `QUILLO_SERVICE_SECRET` cannot be rotated without failing calls: one value, no key id, no previous key accepted

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, ops | a call signed with the next secret | 1 |

```sh
QUILLO_SERVICE_SECRET=the-next-secret-0123456789abcdef01 python3 sign.py http://127.0.0.1:18601 billing refund '[7, 1]'
# 401 {"error":"not signed with this server's QUILLO_SERVICE_SECRET (or too old)"}
```

`secret()` ([src/service.rs:54](../../src/service.rs)) reads one value. Rolling a new one out machine by machine fails every call between a machine that has it and one that does not. A `.later` call waits it out, since a 401 is tried again for a day, but a plain call fails, and so do joins and the channels bridged under `--split`. The other choice is to stop everything and change it at once. A leaked secret is therefore replaced with an outage, or not at all. Sessions have the same shape (`QUILLO_SECRET`), but there a failed check only signs a visitor out.

**Suggestion:** sign with the first and accept any of several: `QUILLO_SERVICE_SECRET=new,old`, or `QUILLO_SERVICE_SECRET_PREVIOUS`. With keys for each service (Q-458), let `keys: { shop: [new, old] }` take a list. The 401's text could say which key id it expected.

### Q-462. Service calls cannot use mutual TLS: neither `service` nor `http.*` can present a client certificate or trust a CA of their own

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading src/fetch.rs and `set_opts` | 0 |

`http.*` takes `redirects`, `public_only`, `cache` and `timeout` ([src/fetch.rs:79](../../src/fetch.rs)). Service calls use an agent with ureq's bundled roots ([src/service.rs](../../src/service.rs) `agent()`). Quillo serves no TLS for pages itself (FEEDBACK36), so mTLS between Quillo services works only with a sidecar doing both ends: Envoy, Linkerd or Istio, which then also does the authorization Quillo cannot (Q-458). That is the usual answer on Kubernetes. It is not one for two VMs, or for a bank or health system that requires client certificates from the application itself. The guide's database section already has `ssl_cert`, `ssl_key` and `ssl_ca` for MySQL, so the vocabulary exists.

**Suggestion:** `auth: { mtls: { cert, key, ca } }` on `service`, and `{ cert, key, ca }` options for `http.*` (ureq 2 with rustls takes a client certificate). On the serving side, a proxy ends TLS, so let `accept_calls { client_cert: { header } }` read the identity from the proxy's header, trusted only from addresses named in `--trust-proxy`.

### Q-463. Plugin `jwt.qwer`: `jwt_verify` refuses every token that has an `aud`, a token with no `exp` is good forever, and neither verifier checks `iss`. Standard service tokens cannot be checked with it

| Kind | Detected by | Attempts |
|---|---|---|
| silent (a token without `exp`), loud (`aud`) | the routes workaround above | 2 |

```quillo
use "jwt.qwer"
let KEY = env("JWT_KEY") ?? ""
print(jwt_verify(jwt_sign({ sub: "service:shop" }, KEY, 0)?, KEY))                // { iat: ..., sub: "service:shop" }: never expires
print(jwt_verify(jwt_sign({ sub: "s", aud: "orders" }, KEY, 60)?, KEY))           // err("InvalidAudience")
```

jsonwebtoken 9 checks `aud` whenever the token has one, and `jwt_verify` takes no audience, so it refuses it. `jwt_verify_rs256(token, pem, "")` does the same. RFC 9068 requires `aud` in an access token, so the plugin cannot check the tokens an identity provider issues for services. `validation()` clears `required_spec_claims`, so `exp` is checked only when present, and `iss` is never checked. Nothing in `jwt.qwer` reads a JWKS or takes ES256 or EdDSA. `oidc.qwer` does read JWKS, but `oidc_verify` is for ID tokens: it asks for a nonce, and the audience is the client id. Neither plugin has the client-credentials grant. That is a plugin's report, not Quillo's, but it is the first thing a reader reaches for after Q-460.

**Suggestion:** `jwt_verify(token, secret, { audience, issuer, require: ["exp"] })`, with `exp` required unless told otherwise. A `jwt_verify_jwks(token, { jwks_url, issuer, audience, algorithms })` for access tokens, sharing `oidc.qwer`'s key cache. `oidc_client_credentials(issuer, client)` returning a token and when it expires. These are also what the built-in `jwt` and `oauth` of the design would be made of.

### Q-464. The guide does not say how to give a service standard authentication today, or what it costs

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading USERGUIDE.md, Services | 0 |

The guide's Routes section shows a bearer check as middleware (`auth.required`). Services says calls are signed with the shared secret and describes the wire "for a service written in another language". Nothing joins the two. A reader who must accept an OAuth token cannot learn from the guide that the only way is routes plus `http.*`, nor that this gives up checks, contracts, retries run once, deadlines, `.later` and published topics. That is the table in "What a program can do now" above.

**Suggestion:** a short "Authentication between services" part in Services: what the secret proves and what it does not (Q-458), the routes way with `jwt.qwer`, and mTLS through a sidecar. Then the design's options, when built.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Looked for "the new feature" in the branch's diff and in the guide, to review it | the request said "review the new feature" | nothing of it exists on main or on `feedback27-35-fixes`: no `auth` option, no JWT or OAuth in src/. Reviewed the current mechanism and wrote the design instead | 3 greps |
| Named the report FEEDBACK37.md | the user said 37 | taken by another session's report; the user corrected it to 38 | 1 `ls` |
| The routes workaround's token had `aud: "orders"` | every standard access token has one | `InvalidAudience`, which is how Q-463 was found; it worked once `aud` was dropped | 1 run |
| Expected `request.headers` inside a `server fn` answering a service, as in a route | routes have it | `request` is only `{ id, call }` (Q-460) | 1 run |

## Limitations

- mTLS was not run: no certificates or proxy were set up. Q-462 comes from reading the client's options and the agent.
- OAuth client credentials were not run against a real identity provider. The design's `oauth` is what the grant needs (a token endpoint, a client id and secret, a scope and an audience), not a tested call.
- `--split` and `--scale` were not tried: they make one secret for their own processes on 127.0.0.1, where Q-458 and Q-459 matter only if something else on the machine reads the environment.

## Edge cases checked that behave well

- A call signed with another secret: 401, with no detail about which part was wrong.
- A call signed for `orders` sent to `billing`: 401 ``this call is for service `orders`; this is `billing` (see --service-name)``.
- The same call id sent twice: the same answer, its request id included, so the second was not run again.
- `QUILLO_SERVICE_SECRET=abc`: a warning at start that it is 3 bytes and should be 32 random bytes, with `openssl rand -hex 32`.
- A join with no nonce is refused, and a join's nonce cannot be used twice (409).
- `GET /_quillo/ready` lists `service billing` as ready without its address. A joined copy's address shows only in the app's log.
- `GET /_quillo/describe` without `--describe`: 404.
- `http.post(url, body, { authorization: ... })` sends the header as given, and its 401 comes back as a value with `status`, `headers` and `json`, not an error, so a caller can tell "refused" from "not reached".

## Probe scripts

`sign.py`, a service call signed as any holder of the secret can sign one (the format from USERGUIDE.md's "On the wire"):

```python
import hmac, hashlib, time, json, sys, os, secrets, urllib.request
def call(base, to, f, args, extra={}, tenant=""):
    secret = os.environ["QUILLO_SERVICE_SECRET"].encode()
    t = int(time.time()); cid = secrets.token_hex(8); body = json.dumps(args).encode()
    msg = f"quillo-service-2\n{t}\n{to}\n{f}\n{tenant}\n{cid}\n".encode() + body
    sig = hmac.new(secret, msg, hashlib.sha256).hexdigest()
    h = {"content-type": "application/json", "x-quillo-service": f"t={t},sig={sig}", "x-quillo-to": to, "x-quillo-call": cid}
    if tenant: h["x-quillo-tenant"] = tenant
    h.update(extra)
    r = urllib.request.Request(f"{base}/_quillo/service/{f}", data=body, headers=h, method="POST")
    try:
        with urllib.request.urlopen(r) as resp: return resp.status, resp.read().decode()
    except urllib.error.HTTPError as e: return e.code, e.read().decode()
base, to, f = sys.argv[1:4]
print(*call(base, to, f, json.loads(sys.argv[4]), json.loads(sys.argv[5]) if len(sys.argv) > 5 else {}))
```

`join.py`, a join as any service:

```python
import hmac, hashlib, time, json, sys, os, secrets, urllib.request
app, service, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
secret = os.environ["QUILLO_SERVICE_SECRET"].encode()
t = int(time.time()); nonce = secrets.token_hex(8); body = json.dumps({"service": service, "port": port}).encode()
msg = f"quillo-service-2\n{t}\n_join\n{service}\n\n{nonce}\n".encode() + body
sig = hmac.new(secret, msg, hashlib.sha256).hexdigest()
r = urllib.request.Request(f"{app}/_quillo/join", data=body, method="POST",
    headers={"content-type": "application/json", "x-quillo-service": f"t={t},sig={sig}", "x-quillo-call": nonce})
print(urllib.request.urlopen(r).read().decode())
```

`fake.py` is a 15-line `http.server` that answers every POST with `{"ok": {"refunded": 0, "by": "an impostor"}}` and prints the path, `x-quillo-to`, `x-quillo-tenant` and the body.
