# Authentication

Signing someone in is two steps. A route checks who the visitor is: a password, a code, a
passkey, an answer from Google. Then it calls `set_session({ user: id })`, and every page,
`server fn` and helper after that reads `session().user`. Quillo's core does passwords,
sessions and calls between services. Every other protocol is a plugin from the
[catalog](/plugins/), and what a plugin keeps (limits, keys, links) goes in tables of the
program's own database. Nothing is limited, remembered or ended on the server unless the
program asks for it: this chapter shows how, one protocol at a time.

## How signing in works

A sign-in form, a route that checks it, a page that needs a visitor, and a way out:

```quillo
db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text)")

route GET /login =
    <html><body>
        <form method="post" action="/login">
            <label>Email <input name="email" type="email" autocomplete="username"/></label>
            <label>Password <input name="password" type="password" autocomplete="current-password"/></label>
            <button>Sign in</button>
        </form>
    </body></html>

route POST /login { form: { email: str, password: str } } {
    let user = db.one("select id, password_hash from users where email = ?", lower(normalize(trim(email))))
    if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }
    set_session({ user: user.id })
    redirect("/")
}

route GET / {
    let id = session().user ?? reply 303 redirect("/login")
    <html><body>
        <p>Signed in as user {id}.</p>
        <form method="post" action="/logout"><button>Sign out</button></form>
    </body></html>
}

route POST /logout { end_session(); redirect("/login") }
```

- `session().user` is the one name every feature reads: notifications, the MCP door,
  tenants' pages. There is no `user()` builtin and no users table of Quillo's: the table is
  yours, and so is what `user` holds (an id, a DN, an email).
- An email nobody has finds no row, so `user.password_hash` is null, and
  `crypto.verify_password` says false after the same work as for a real hash: the answer
  takes as long, and tells no one which emails have accounts.
- Signing out is a form that posts. A link would be a `GET`, which another site, a
  prefetch or a mail scanner could follow. The cookie and its rules are in [Headers,
  cookies and sessions](#sessions).

**Plugins.** `quillo install login_limits` puts `login_limits.qwer` beside the program, its
version and hash checked against the catalog; `quillo install --update` brings the newer
ones. `use "login_limits.qwer"` then builds it with cargo the first time the program runs
(see [Rust](#rust), and `quillo build` for servers without Rust). Three things hold
for every plugin in this chapter:

- **A route that calls one has no `with`.** The catalog's files do not yet say what they do
  (an `//! effects:` line), so `quillo check` refuses them in a function whose `with`
  limits what it may do.
- **A plugin never opens the database.** Those that keep data give the SQL of their table
  (`login_limits_table(config)`) for SQLite, PostgreSQL or MySQL, and each call gives the
  statements to run as `{ steps, atomic }`. The program runs them, so the rows sit beside
  its own, in one backup, on every server.
- **Secrets come from the environment.** The plugins that keep keys or links take a
  `secret` of 32 characters or more (`openssl rand -hex 32`). Each derives keys of its own
  from it, so one `AUTH_SECRET` serves them all. Keep it as you keep `QUILLO_SECRET`:
  changing it ends every API key and every link not yet used.

## What to use for each protocol

| Protocol | What to use | Section |
|---|---|---|
| Passwords | Core: `crypto.hash_password`, `crypto.verify_password` (Argon2id) | [Passwords](#passwords) |
| Users moved from PHP, Rails, Django, WordPress or Firebase | Plugins: [bcrypt](/plugins/bcrypt/), [scrypt](/plugins/scrypt/), [legacy_passwords](/plugins/legacy_passwords/), [argon2](/plugins/argon2/) | [Passwords](#passwords) |
| Password strength, breached passwords, passphrases | Plugins: [password_strength](/plugins/password_strength/), [pwned](/plugins/pwned/), [passphrase](/plugins/passphrase/) | [Passwords](#passwords) |
| Sessions, signing out, signing out everywhere | Core: `set_session`, `session()`, `end_session()`; a sessions table of yours | [Sessions and signing out](#sessions-and-signing-out) |
| Limits on guessing | Plugin: [login_limits](/plugins/login_limits/) | [Limits on guessing](#limits-on-guessing) |
| Two-factor codes (TOTP) and recovery codes | Plugins: [totp](/plugins/totp/), [mfa_codes](/plugins/mfa_codes/); YubiKey OTP: [yubico_otp](/plugins/yubico_otp/) | [Two-factor sign-in](#two-factor-sign-in) |
| Passkeys (WebAuthn, FIDO2 keys) | Plugin: [passkeys](/plugins/passkeys/), and `js()` in a component | [Passkeys](#passkeys) |
| Sign in with Google, Microsoft, or any OpenID Connect provider | Plugin: [oidc](/plugins/oidc/) | [Sign in with Google, GitHub, Microsoft or Apple](#sign-in-with-google-github-microsoft-or-apple) |
| Sign in with GitHub (OAuth 2.0) | Plugin: [oauth_pkce](/plugins/oauth_pkce/) | the same |
| Sign in with Apple | Plugins: [oidc](/plugins/oidc/), and [jwt](/plugins/jwt/) for its client secret | the same |
| Email links and codes, SMS codes | Plugin: [magic_links](/plugins/magic_links/), sent with [smtp](/plugins/smtp/), [mail_api](/plugins/mail_api/) or [sms](/plugins/sms/) | [Email links and codes](#email-links-and-codes) |
| API keys | Plugin: [api_keys](/plugins/api_keys/) | [API keys](#api-keys) |
| AI clients at the MCP door | Core: a signed-in session, or `mcp_token()` | [API keys](#api-keys) |
| JWT, JWKS, PASETO, Biscuit | Plugins: [jwt](/plugins/jwt/), [jwks](/plugins/jwks/), [paseto](/plugins/paseto/), [biscuit](/plugins/biscuit/) | [JWT, JWKS and PASETO](#jwt-jwks-and-paseto) |
| HTTP Basic and Digest | Plugin: [http_auth](/plugins/http_auth/) | [HTTP Basic and Digest](#http-basic-and-digest) |
| SAML 2.0 | Plugin: [saml](/plugins/saml/) | [SAML](#saml) |
| LDAP and Active Directory | Plugin: [ldap](/plugins/ldap/) | [LDAP and Active Directory](#ldap-and-active-directory) |
| RADIUS | Plugin: [radius](/plugins/radius/) | [RADIUS](#radius) |
| Calls between services | Core: `service ... { auth }` and `accept_calls` | [Calls between services](#calls-between-services) |
| Client certificates (mTLS), Kerberos | Core: a proxy in front checks them, and `--client-cert-header` and `--user-header` read what it says *(since 0.8.9)* | [Client certificates and Kerberos](#client-certificates-and-kerberos) |
| An OAuth 2.0 authorization server | Not available yet: planned as a plugin | [An OAuth authorization server](#an-oauth-authorization-server) |

Core has no TOTP and no rate limit of its own: `crypto.totp_*` and `rate_limit` were
removed in 0.5.0, and calling them says so.

## Passwords

`crypto.hash_password(password)` makes an Argon2id hash with a salt of its own
(`$argon2id$v=19$m=19456,t=2,p=1$...`, OWASP's floor), and
`crypto.verify_password(password, hash)` checks one. Both run off the event loop, one hash
per core at most, so a burst of sign-ins slows sign-ins, not the site. Store the hash
alone, never the password.

A sign-up that refuses passwords that are easy to guess, or that appear in known breaches:

```quillo
use "password_strength.qwer"
use "pwned.qwer"

db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text)")

// How often a password appears in breaches, from Have I Been Pwned: only the first five
// characters of its SHA-1 leave the server. Null when the service could not be asked.
fn breached(password: str) {
    let r = http.get(pwned_range(password).url, { "add-padding": "true" }, { public_only: true })
    if is_err(r) || r.status != 200 { return null }
    let n = pwned_count(password, r.body)
    is_err(n) ? null : n
}

route POST /signup { form: { email: str, password: str } } {
    let address = lower(normalize(trim(email)))
    if len(password) < 8 { reply 400 "Use 8 characters or more" }
    let verdict = password_check(password, [address, "Corner Shop"])
    if verdict.score < 3 { reply 400 `Too easy to guess. ${join(verdict.suggestions, " ")}` }
    if (breached(password) ?? 0) > 0 { reply 400 "That password is in known data breaches: choose another" }
    db.exec("insert into users (email, password_hash) values (?, ?) on conflict (email) do nothing", address, crypto.hash_password(password))
    redirect("/login")
}
```

- Store an email the way it is looked up: trimmed, in lower case, in one Unicode form.
- `password_check` judges with zxcvbn, from 0 (guessed at once) to 4. The words given
  (the email, the site's name) are the ones a guesser tries first.
- A sign-up for an email that has an account changes nothing here and answers the same, so
  the form tells no one who is registered. Send that address an email instead ("you
  already have an account"), with [Email links and codes](#email-links-and-codes).
- `passphrase(5, "-")` makes a first password people can type
  (`crane-shrunk-gristle-unwind-pacific`, 64 bits).

### Users moved from another system

Hashes imported from another system (bcrypt from PHP, Rails or Laravel; Django's PBKDF2,
Flask's and WordPress's; scrypt) are checked by their plugin, and replaced by an Argon2id
hash the first time the password is right:

```quillo
use "bcrypt.qwer"
use "scrypt.qwer"
use "legacy_passwords.qwer"

db.open("app.db")

// Whether `password` is right for `stored`, and whether the hash is an old kind to replace.
fn check_password(password: str, stored) -> map {
    if stored == null || starts_with(stored, "$argon2") { return { ok: crypto.verify_password(password, stored), old: false } }
    if is_bcrypt(stored) { return { ok: bcrypt_verify(password, stored), old: true } }
    if starts_with(stored, "$scrypt$") { return { ok: scrypt_verify(password, stored), old: true } }
    let r = legacy_verify(password, stored)               // Django, Werkzeug, WordPress
    { ok: !is_err(r) && r, old: true }
}

route POST /login { form: { email: str, password: str } } {
    let started = now()
    let user = db.one("select id, password_hash from users where email = ?", lower(normalize(trim(email))))
    let r = check_password(password, user.password_hash)
    if !r.ok || user == null {
        sleep(max([0, 400 - (now() - started)]))          // every failure answers at one time
        reply 401 "Wrong email or password"
    }
    if r.old { db.exec("update users set password_hash = ? where id = ?", crypto.hash_password(password), user.id) }
    set_session({ user: user.id })
    redirect("/")
}
```

- A bcrypt check (about 250 ms at cost 12) or Django's 600,000 rounds take far longer than
  an Argon2id one (about 10 ms). Without the wait, the time of a failure would tell whether
  an account exists and whether it has moved yet. Set it above the slowest check your
  imported hashes take.
- bcrypt reads only the first 72 bytes of a password, as the old system did, so a longer
  one matches any password that shares them. Rehashing on success ends that for the user.
  `bcrypt_hash` refuses a password over 72 bytes *(since bcrypt 2.0.0: it hashed the first
  72)*: make new hashes with `crypto.hash_password`.
- Firebase exports use its modified scrypt: `scrypt_firebase_verify`, with the project's
  hash parameters. `legacy_verify` caps PBKDF2 at 5 million rounds, so a hash planted in
  the table cannot hold a core.
- Argon2 hashes made elsewhere with other costs (PHP's `password_hash`, libsodium) are read
  by `argon2_verify`; `argon2_needs_rehash` says when one costs less than you want.

## Sessions and signing out

A session is a map in a cookie the server signs: `__Host-quillo_session`, `HttpOnly`,
`Secure`, `SameSite=Lax`, lasting 30 days unless `set_session(map, { max_age })` says
otherwise. The browser can read it but not change it, so it holds an id and never a secret
(not a provider's token either). Set `QUILLO_SECRET` to 32 random bytes or more, the same on
every server; [Headers, cookies and sessions](#sessions)
has the rest.

`end_session()` removes the cookie from this browser, and ends the session's MCP tokens and
notifications. A copy of the cookie kept elsewhere (a stolen laptop, malware, a log) stays
valid until it expires: Quillo keeps no list of sessions, and has no "sign out everywhere"
of its own. A program that needs one keeps a row per sign-in and checks it:

```quillo
db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text)")
db.exec("create table if not exists sessions (id text primary key, user_id integer not null, created integer not null)")

// A row of the program's own for each sign-in; the cookie holds its id.
fn sign_in(user_id: int) {
    let sid = crypto.token()
    db.exec("insert into sessions (id, user_id, created) values (?, ?, ?)", sid, user_id, now())
    set_session({ user: user_id, sid })
}

// The signed-in user while their row is there: deleting it ends every copy of that cookie.
fn me() {
    let s = session()
    if s.sid == null { return null }
    db.one("select user_id from sessions where id = ?", s.sid).user_id
}

route POST /login { form: { email: str, password: str } } {
    let user = db.one("select id, password_hash from users where email = ?", lower(normalize(trim(email))))
    if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }
    sign_in(user.id)
    redirect("/")
}

route GET /account {
    let id = me() ?? reply 303 redirect("/login")
    <html><body><p>Account {id}</p><form method="post" action="/logout/everywhere"><button>Sign out everywhere</button></form></body></html>
}

route POST /logout {
    db.exec("delete from sessions where id = ?", session().sid)
    end_session()
    redirect("/login")
}

route POST /logout/everywhere {
    let id = me() ?? reply 401 "Sign in first"
    db.exec("delete from sessions where user_id = ?", id)
    end_session()
    redirect("/login")
}
```

- Read the user with `me()`, not `session().user`, wherever an ended session must stop
  working. It costs one indexed read per request.
- When a password changes, delete the user's other rows (`where user_id = ? and id != ?`),
  so a thief signed in elsewhere is out.
- Sessions renew only when `set_session` is called again; [Headers, cookies and
  sessions](#sessions) shows a sliding session.

## Limits on guessing

Quillo limits nothing by itself. A route that checks a password, a code or a key takes as
many tries as it is sent: one kept connection makes about 20,000 a second. The
[login_limits](/plugins/login_limits/) plugin counts attempts in a table of the program's
database, so every instance and server shares the counts and a restart forgets none:

```quillo
use "login_limits.qwer"

db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text)")
let LIMITS = { table: "login_limits", database: "sqlite", secret: env("AUTH_SECRET") }
db.exec(login_limits_table(LIMITS)?)

// Runs a plugin's statements: the last row. MySQL's come `atomic`, for one short transaction.
fn run_steps(steps: list) {
    let row = null
    for s in steps { row = db.one(s.sql, s.args)? }
    row
}
fn run_sql(q: map) = q.atomic ? db.transaction({ retries: 3 }, () => run_steps(q.steps)) : run_steps(q.steps)

// Counts one attempt against each limit in turn: the seconds to wait, or 0.
fn wait_for(limits: list) -> int {
    for l in limits {
        let r = run_sql(login_limits_take(LIMITS, l.kind, l.key)?)?
        if r.allowed == 0 { return r.retry_after }
    }
    0
}

route POST /login { form: { email: str, password: str } } {
    let typed = lower(normalize(trim(email)))
    let user = db.one("select id, password_hash from users where email = ?", typed)
    let account = user == null ? `email:${typed}` : `user:${user.id}`
    let wait = wait_for([{ kind: "address", key: request.ip }, { kind: "account", key: account }])
    if wait > 0 { reply 429 respond(429, `Too many tries: try again in ${wait} s`, { "retry-after": str(wait) }) }
    if !crypto.verify_password(password, user.password_hash) || user == null { reply 401 "Wrong email or password" }
    run_sql(login_limits_clear(LIMITS, "account", account)?)?     // the account starts again
    run_sql(login_limits_refund(LIMITS, "address", request.ip)?)? // and the address gets its try back
    set_session({ user: user.id })
    redirect("/")
}

every(600000, () => { run_sql(login_limits_sweep(LIMITS)?) })   // rows with every try back
```

The table it needs, as `login_limits_table` writes it for SQLite (PostgreSQL's is the same;
MySQL's adds a binary collation):

```sql
create table if not exists login_limits (k varchar(64) primary key, tat bigint not null, ok integer not null)
```

- **The limits.** `account`: 5 tries per 15 minutes. `address`: 50, higher for an office or a
  phone network behind one address; an IPv6 address counts by its /64, which its owner can
  fill at will (`ipv6_prefix: 56` or `48` for wider). `code`: 3, for 6-digit codes. A limit of
  your own goes in `limits: { send: { tries: 3, per_seconds: 3600 } }`.
- **Count first, then check.** Each attempt is one statement (an upsert, `returning` its
  answer), so fifty guesses sent at once get five tries, not fifty.
- **Tries come back one at a time**, one every 3 minutes for an account: a full limit slows
  its key down and never locks it for good, so someone using up an account's tries cannot
  shut its owner out, and an email link still signs them in.
- **Count an account by its id** once found. A database collation that takes `àdä@` for
  `ada@` finds Ada's row for both, and two spellings must not get two counts. An email
  nobody has is counted by itself, so it behaves the same and tells no one it is unknown.
- **Behind a proxy, start Quillo with `--trust-proxy`** naming it (`quillo run main.qwe
  --trust-proxy 127.0.0.1`). Without it, `request.ip` is the proxy's for every visitor, and
  they all share one address limit.
- **Keys are HMACs** under `secret`, so the table holds no email or address anyone can read
  back. Without a secret they are plain SHA-256, which an IPv4 address can be found from.
- On PostgreSQL or MySQL, say so: `database: "postgres"` (or the URL given to `db.open`).
  The calls stay the same; the plugin writes that database's SQL.

## Two-factor sign-in

After the password, a 6-digit code from an authenticator app (TOTP,
[totp](/plugins/totp/)), or one of ten recovery codes for a lost phone
([mfa_codes](/plugins/mfa_codes/)). Turning it on is two posts: one shows a new secret, the
next takes a code made from it and only then keeps it. Signing in keeps the visitor
`pending` between the two steps:

```quillo
use "totp.qwer"
use "mfa_codes.qwer"
use "login_limits.qwer"

db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text, totp_secret text, totp_new text, totp_step integer not null default -1)")
db.exec("create table if not exists recovery_codes (id integer primary key, user_id integer not null, hash text not null)")
let LIMITS = { secret: env("AUTH_SECRET") }
db.exec(login_limits_table(LIMITS)?)

fn run_sql(q: map) {
    let row = null
    for s in q.steps { row = db.one(s.sql, s.args)? }
    row
}

route POST /settings/2fa/start {
    let me = session().user ?? reply 401 "Sign in first"
    let secret = totp_secret()
    db.exec("update users set totp_new = ? where id = ?", secret, me)
    let email = db.one("select email from users where id = ?", me).email
    <html><body>
        <p>Add this to your authenticator app: <a href={totp_url(secret, "Corner Shop", email)?}>open it on this phone</a>, or type {secret}.</p>
        <form method="post" action="/settings/2fa/confirm">
            <input name="code" inputmode="numeric" autocomplete="one-time-code"/>
            <button>Turn on</button>
        </form>
    </body></html>
}

route POST /settings/2fa/confirm { form: { code: str } } {
    let me = session().user ?? reply 401 "Sign in first"
    let r = totp_check(db.one("select totp_new from users where id = ?", me).totp_new ?? "", code, -1)
    if is_err(r) || !r.ok { reply 400 "That code is not right: try the next one" }
    db.exec("update users set totp_secret = totp_new, totp_new = null, totp_step = ? where id = ?", r.step, me)
    let codes = mfa_codes(10)
    db.exec("delete from recovery_codes where user_id = ?", me)
    for c in codes { db.exec("insert into recovery_codes (user_id, hash) values (?, ?)", me, mfa_hash(c)?) }
    <html><body>
        <p>Two-factor sign-in is on. Keep these codes with your passwords: each works once, and they are shown only now.</p>
        <ul>{for c in codes => <li><code>{c}</code></li>}</ul>
    </body></html>
}

route POST /login { form: { email: str, password: str } } {
    let user = db.one("select id, password_hash, totp_secret from users where email = ?", lower(normalize(trim(email))))
    if !crypto.verify_password(password, user.password_hash) { reply 401 "Wrong email or password" }
    if user.totp_secret != null {
        set_session({ pending: user.id }, { max_age: "5m" })    // not `user`: nothing counts it as signed in
        reply 303 redirect("/login/code")
    }
    set_session({ user: user.id })
    redirect("/")
}

// A code from the app, taken once: its time step is kept, and a code of that step or an
// earlier one is refused, though it stays right for up to 90 s.
fn totp_ok(id: int, code: str) -> bool {
    let u = db.one("select totp_secret, totp_step from users where id = ?", id)
    let r = totp_check(u.totp_secret ?? "", code, u.totp_step)
    if is_err(r) || !r.ok { return false }
    db.exec("update users set totp_step = ? where id = ? and totp_step < ?", r.step, id, r.step).changes == 1
}

// A recovery code: deleted by the one request that matched it first.
fn recovery_ok(id: int, code: str) -> bool {
    let rows = db.query("select id, hash from recovery_codes where user_id = ? order by id", id)
    let at = mfa_check(code, rows | map((r) => r.hash))
    at >= 0 && db.exec("delete from recovery_codes where id = ?", rows[at].id).changes == 1
}

route POST /login/code { form: { code: str } } {
    let id = session().pending ?? reply 401 "Sign in with your password first"
    let limit = run_sql(login_limits_take(LIMITS, "code", `user:${id}`)?)?
    if limit.allowed == 0 { reply 429 respond(429, `Too many tries: try again in ${limit.retry_after} s`, { "retry-after": str(limit.retry_after) }) }
    if !totp_ok(id, code) && !recovery_ok(id, code) { reply 401 "Wrong code" }
    run_sql(login_limits_clear(LIMITS, "code", `user:${id}`)?)?
    set_session({ user: id })
    redirect("/")
}
```

- **A code is taken once.** RFC 6238 says so, and a code seen over a shoulder or through a
  phishing proxy is otherwise good for 90 s. `totp_check(secret, code, last_step)` gives
  `{ ok, step, reused }` *(since totp 1.1.0: `totp_verify` gave only true or false, and
  still does)*. The `update ... where totp_step < ?` keeps the step only for the first of
  two requests that race.
- **Codes are limited.** There are a million codes and three are right at any moment, so
  unlimited guessing finds one in seconds. The `code` limit allows 3 tries per 15 minutes,
  then one every 5. Count the password step too, as [Limits on
  guessing](#limits-on-guessing) does.
- **Recovery codes** are ten characters (`xxxxx-xxxxx`, about 49 bits), shown once. Each is
  stored as an Argon2id hash with a salt of its own *(since mfa_codes 2.0.0: an unsalted
  SHA-256, so one pass over every code gave every user's from a leaked table)*.
  `mfa_check` checks every stored hash (about 25 ms each), whichever matches, so its time
  tells nothing. Spaces, dashes and capitals do not count, as people copy them from paper.
- `qr_svg(totp_url(...)?, 200)` from the [qr](/plugins/qr/) plugin draws the link as a QR
  code for the set-up page, in `raw(...)`.
- A TOTP secret is as good as the factor itself. It sits in your table in plain text, as
  the app holds it: keep the table's backups as you keep `AUTH_SECRET`.
- [yubico_otp](/plugins/yubico_otp/) checks a YubiKey's one-time password, with YubiCloud
  or a key of your own. Tie the key's public id to the user: any valid YubiKey passes.

## Passkeys

A passkey is a key pair the browser or a security key keeps for one site, unlocked by a
fingerprint, a face or a PIN. The [passkeys](/plugins/passkeys/) plugin checks the
browser's answers on the server; the browser half is three `js()` calls in a component.
Challenges go in a table, so each is used once, on every server:

```quillo
use "passkeys.qwer"

db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text)")
db.exec("create table if not exists passkeys (credential_id text primary key, user_id integer not null, public_key text not null, counter integer not null)")
db.exec("create table if not exists passkey_challenges (challenge text primary key, user_id integer, expires integer not null)")

let SITE = { id: "shop.example", name: "Corner Shop" }
let WANT = { origin: "https://shop.example", rp_id: "shop.example" }

fn new_challenge(o: map, user_id) -> map {
    db.exec("delete from passkey_challenges where expires < ?", now())
    db.exec("insert into passkey_challenges (challenge, user_id, expires) values (?, ?, ?)", o.challenge, user_id, now() + 300000)
    o
}

// The challenge's user, the first time it is given back; an error after that.
fn use_challenge(challenge: str) {
    let row = db.one("select user_id from passkey_challenges where challenge = ? and expires > ?", challenge, now())
    if row == null || db.exec("delete from passkey_challenges where challenge = ?", challenge).changes != 1 { return err("Start again") }
    row
}

server fn passkey_new() -> map {
    let me = session().user ?? return err("Sign in first")
    let email = db.one("select email from users where id = ?", me).email
    let have = db.query("select credential_id from passkeys where user_id = ?", me) | map((r) => r.credential_id)
    new_challenge(passkeys_register_options(SITE, { name: email, display_name: email }, { exclude: have })?, me)
}

server fn passkey_add(challenge: str, credential: map) -> map {
    let me = session().user ?? return err("Sign in first")
    if use_challenge(challenge)?.user_id != me { return err("Start again") }
    let k = passkeys_register_verify(credential, { ...WANT, challenge })
    if is_err(k) {
        print(`passkey not added: ${error(k)}`)
        return err("This passkey could not be added")
    }
    db.exec("insert into passkeys (credential_id, user_id, public_key, counter) values (?, ?, ?, ?)", k.credential_id, me, k.public_key, k.counter)
    { added: true }
}

// Any passkey of this site: the browser offers the visitor theirs, no name typed.
server fn passkey_sign_in_options() -> map = new_challenge(passkeys_signin_options(SITE.id, [], {})?, null)

server fn passkey_sign_in(challenge: str, credential: map) -> map {
    use_challenge(challenge)?
    let kept = db.one("select credential_id, user_id, public_key, counter from passkeys where credential_id = ?", credential.id) ?? return err("This passkey is not one of ours")
    let r = passkeys_signin_verify(credential, kept, { ...WANT, challenge })
    if is_err(r) {
        print(`passkey sign-in refused: ${error(r)}`)
        return err("That passkey did not sign you in")
    }
    db.exec("update passkeys set counter = ? where credential_id = ?", r.counter, kept.credential_id)
    set_session({ user: kept.user_id })
    { ok: true }
}

component PasskeyButtons() {
    state note = ""
    <div>
        <button @click={() => {
            let o = passkey_new()
            if is_err(o) { note = o.error; return }
            let cred = js("navigator.credentials.create", { publicKey: js("PublicKeyCredential.parseCreationOptionsFromJSON", o) })
            if is_err(cred) { note = "No passkey was made"; return }
            let r = passkey_add(o.challenge, json.parse(js("JSON.stringify", cred))?)
            note = is_err(r) ? r.error : "Passkey added"
        }}>Add a passkey</button>
        <button @click={() => {
            let o = passkey_sign_in_options()
            let cred = js("navigator.credentials.get", { publicKey: js("PublicKeyCredential.parseRequestOptionsFromJSON", o) })
            if is_err(cred) { note = "No passkey was given"; return }
            let r = passkey_sign_in(o.challenge, json.parse(js("JSON.stringify", cred))?)
            if is_err(r) { note = r.error } else { navigate("/") }
        }}>Sign in with a passkey</button>
        <p>{note}</p>
    </div>
}

route GET /passkeys = <html><body><PasskeyButtons/></body></html>
```

- **The credential goes back through `JSON.stringify`.** A browser's credential keeps its
  fields on its prototype, and on 0.8.8 a page's object passed to a `server fn` by its own
  keys, so it arrived empty (`the credential's type is null`). `JSON.stringify` calls its
  `toJSON()`, which is what the plugin reads. Since 0.8.9 an object with a `toJSON`, passed
  as it is, goes as its `toJSON()` gives, as `JSON.stringify` writes it; the round trip
  here works on both.
- **`origin` and `rp_id` are exact.** The origin is the page's (`https://shop.example`, a
  list for several), the `rp_id` its host or a parent of it. While developing, both are
  `localhost`'s (`http://localhost:8080` and `localhost`): browsers allow passkeys there
  without HTTPS.
- **User verification is required** *(since passkeys 2.0.0: it was checked only when
  asked)*, so a found security key without a PIN cannot sign in alone. Turn it off with
  `require_user_verification: false` in `WANT` only where the passkey is a second factor
  after a password.
- **The plugin's reasons go to the log.** An error value a plugin gives reaches the page as
  `internal error`, so the functions answer in words of their own and print the reason.
- A challenge is good once, for five minutes, from the table, on every server: a captured
  answer cannot be sent again. A counter that does not go up is refused too, but synced
  passkeys always count 0, so the table is what stops replays. Kept in the session instead,
  a challenge would live on in every copy of the cookie.
- Ask for `attestation: "none"` (the default). A `packed` attestation signed by a
  certificate is `unverified-certificate` *(since passkeys 2.0.0: `basic`)*: its chain is
  not checked, and it proves nothing about the device until you check it against its
  maker's root.

## Sign in with Google, GitHub, Microsoft or Apple

Google, Microsoft and Apple speak OpenID Connect, which the [oidc](/plugins/oidc/) plugin
does whole: discovery, state, nonce and PKCE, the code exchanged, and the ID token checked
against the provider's keys (RS256 or ES256, issuer, audience, expiry, nonce). A user is the
provider's `iss` and `sub` together, in a table of yours:

```quillo
use "oidc.qwer"

db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text, name text)")
db.exec("create table if not exists logins (issuer text not null, sub text not null, user_id integer not null, primary key (issuer, sub))")

let GOOGLE = "https://accounts.google.com"
let CLIENT = {
    client_id: env("GOOGLE_CLIENT_ID", ""),
    client_secret: env("GOOGLE_CLIENT_SECRET", ""),
    redirect_uri: "https://shop.example/auth/google",
}

route GET /login/google {
    let s = oidc_start(GOOGLE, CLIENT, {})?
    set_session({ state: s.state, nonce: s.nonce, code_verifier: s.code_verifier }, { max_age: "10m" })
    redirect(s.url)
}

// The provider sends the visitor back here: the session it left with holds the flow.
route writes GET /auth/google {
    let done = oidc_finish(GOOGLE, CLIENT, request.query, session(), {})
    if is_err(done) {
        end_session()
        reply 401 "The sign-in did not work: try again."
    }
    set_session({ user: user_for(done.claims) })
    redirect("/")
}

// The user signed in as `iss` and `sub` before, or a new one. An email is kept only when the
// provider says it is verified, and never used to find an account.
fn user_for(c: map) -> int {
    let found = db.one("select user_id from logins where issuer = ? and sub = ?", c.iss, c.sub)
    if found != null { return found.user_id }
    let id = db.exec("insert into users (email, name) values (?, ?)", c.email_verified == true ? c.email : null, c.name).last_id
    db.exec("insert into logins (issuer, sub, user_id) values (?, ?, ?)", c.iss, c.sub, id)
    id
}
```

- **Never link accounts by email.** A provider lets people set an unverified address, and
  some let a company's admin set anyone's; signing in by it is a takeover. Link a second
  provider to an account from inside that account.
- The return is a `GET` the provider's page sends the browser to, which carries the
  `SameSite=Lax` session, so the flow can wait there; `writes` says the write is meant (see
  [Requests from other sites](#requests-from-other-sites)).
- Keep the provider's tokens out of the session, which the browser can read. A program
  that calls the provider later keeps them in a table, by user.

**Microsoft.** `https://login.microsoftonline.com/<tenant>/v2.0` is one company's directory.
For any work or school account (`organizations`) or any account (`common`), the issuer is a
template, `.../{tenantid}/v2.0`, and any company's users can get a token there, so the check
must say which companies it takes:

```quillo
use "oidc.qwer"

let MICROSOFT = "https://login.microsoftonline.com/organizations/v2.0"
let MS_CLIENT = { client_id: env("MS_CLIENT_ID", ""), client_secret: env("MS_CLIENT_SECRET", ""), redirect_uri: "https://shop.example/auth/microsoft" }

route GET /login/microsoft {
    let s = oidc_start(MICROSOFT, MS_CLIENT, {})?
    set_session({ state: s.state, nonce: s.nonce, code_verifier: s.code_verifier }, { max_age: "10m" })
    redirect(s.url)
}

route writes GET /auth/microsoft {
    let done = oidc_finish(MICROSOFT, MS_CLIENT, request.query, session(), { tenants: [env("MS_TENANT_ID", "")] })
    if is_err(done) {
        end_session()
        reply 401 "The sign-in did not work: try again."
    }
    set_session({ user: `${done.claims.tid}/${done.claims.oid}` })    // a Microsoft user is tid and oid
    redirect("/")
}
```

`tenants` lists the companies' tenant ids taken; `any_tenant: true` takes every company,
for a site open to all. With neither, a template issuer is refused, and so is `tenants`
given as text, or misspelled: options are checked *(since oidc 1.2.0: the template issuer
was refused at discovery, so these sign-ins could not be done)*. A personal account is
refused at `organizations`.

**GitHub** is OAuth 2.0, not OpenID Connect: there is no ID token to check.
[oauth_pkce](/plugins/oauth_pkce/) makes the state, the PKCE pair, the link and the token
request; the program then asks GitHub's API who the visitor is, and keys them by GitHub's
numeric `id`. Its example keeps the access token in a table, by user, with only the user's id
in the session. For an email, read `/user/emails` and keep only an address with `verified:
true`.

**Apple** is OpenID Connect with two turns. Its client secret is a token you sign ES256 with
the `.p8` key Apple gives you, which jwt's `jwt_sign_es256` makes *(since jwt 3.0.0)*. And
when the name or email is asked for, Apple sends the visitor back with a form posted from
its own site, which carries no `SameSite=Lax` cookie: the flow waits in a table, found by a
cookie of its own that is `SameSite=None`, as for [SAML](#saml):

```quillo
use "oidc.qwer"
use "jwt.qwer"

db.open("app.db")
db.exec("create table if not exists apple_flows (state text primary key, nonce text not null, code_verifier text not null, expires integer not null)")
allow_cross_site("/auth/apple")                    // Apple's page posts the answer from its site

let APPLE = "https://appleid.apple.com"

// Apple's client secret: signed with the .p8 key, for at most six months; a day here.
fn apple_client() = {
    client_id: env("APPLE_CLIENT_ID", ""),           // the Services ID: com.example.shop.web
    client_secret: jwt_sign_es256({ iss: env("APPLE_TEAM_ID", ""), sub: env("APPLE_CLIENT_ID", ""), aud: APPLE }, env("APPLE_KEY", ""), env("APPLE_KEY_ID", ""), 86400)?,
    redirect_uri: "https://shop.example/auth/apple",
    scope: "openid email name",
}

route GET /login/apple {
    let s = oidc_start(APPLE, apple_client(), { extra: { response_mode: "form_post" } })?
    db.exec("delete from apple_flows where expires < ?", now())
    db.exec("insert into apple_flows (state, nonce, code_verifier, expires) values (?, ?, ?, ?)", s.state, s.nonce, s.code_verifier, now() + 600000)
    set_cookie("__Host-apple", s.state, { same_site: "None", max_age: 600 })
    redirect(s.url)
}

route POST /auth/apple { form: { code: str, state: str, user: str = "" } } {
    let held = cookie("__Host-apple") ?? ""
    delete_cookie("__Host-apple", { same_site: "None" })
    if held == "" || !crypto.equal(held, state) { reply 401 "This sign-in was not started in this browser" }
    let flow = db.one("select state, nonce, code_verifier from apple_flows where state = ? and expires > ?", state, now())
    if flow == null || db.exec("delete from apple_flows where state = ?", state).changes != 1 { reply 401 "That sign-in was used already" }
    let done = oidc_finish(APPLE, apple_client(), { code, state }, flow, {})
    if is_err(done) { reply 401 "The sign-in did not work: try again." }
    set_session({ user: `apple/${done.claims.sub}` })
    redirect("/")
}
```

- The cookie ties the answer to the browser that started the sign-in, so someone cannot
  have a visitor's browser post an answer they got for their own account.
- Apple sends the person's name only the first time, in the `user` field (JSON), outside
  the ID token: keep it then.
- Apple asks iPhone apps that offer other social sign-ins to offer it too.

## Email links and codes

A link and a 6-digit code, sent to the address, each good once for 15 minutes:
[magic_links](/plugins/magic_links/) makes them, keeps only their HMACs in your table, and
uses them up in one statement. The sending is the program's, by
[smtp](/plugins/smtp/), [mail_api](/plugins/mail_api/) or, for a code by text message,
[sms](/plugins/sms/):

```quillo
use "magic_links.qwer"
use "smtp.qwer"

db.open("app.db")
db.exec("create table if not exists users (id integer primary key, email text unique not null, password_hash text)")
let MAGIC = { secret: env("AUTH_SECRET"), tries: 5, minutes: 15, link: "https://shop.example/login/link" }
db.exec(magic_links_table(MAGIC)?)

fn run_sql(q: map) {
    let row = null
    for s in q.steps { row = db.one(s.sql, s.args)? }
    row
}

// Uses a link ({ token, browser }) or a code ({ address, code }), once.
fn redeem(input: map) -> map {
    let q = magic_links_redeem(MAGIC, input)
    if is_err(q) { return { ok: false, reason: error(q), other_browser: false } }
    let r = magic_links_result(MAGIC, input, run_sql(q)?)?
    if r.ok { run_sql(r)? }                         // ends the email's other link or code
    r
}

fn user_for(email: str) -> int {
    db.exec("insert into users (email) values (?) on conflict (email) do nothing", email)
    db.one("select id from users where email = ?", email).id
}

// The same answer whether the address has an account or not.
route POST /login/email { form: { email: str } } {
    let browser = crypto.token()
    let s = magic_links_start(MAGIC, email, "both", { browser })
    if is_err(s) { reply 400 error(s) }
    run_sql(s)?
    send_email(s.address, "Your sign-in link", `Sign in: ${s.link}\n\nOr type this code: ${s.code}\n\nEach works once, for ${s.minutes} minutes.`, "")?
    set_cookie("sign_in_browser", browser, { max_age: s.minutes * 60 })
    "Check your email: the link and the code work once."
}

// The link's page: the token after the # goes into the form, and leaves the address bar.
route GET /login/link =
    <html><head><meta name="referrer" content="no-referrer"/></head><body>
        <form method="post" action="/login/link">
            <input type="hidden" id="token" name="token" value=""/>
            <button>Sign in</button>
        </form>
        <script>{raw("document.getElementById('token').value = location.hash.slice(1); history.replaceState(null, '', location.pathname)")}</script>
    </body></html>

route POST /login/link { form: { token: str = "", confirm: str = "" } } {
    let r = redeem({ token, browser: cookie("sign_in_browser") ?? "", any_browser: confirm == "yes" })
    if r.other_browser {
        reply 409 <html><body>
            <p>Sign in as {r.masked} in this browser?</p>
            <form method="post" action="/login/link">
                <input type="hidden" name="token" value={token}/>
                <input type="hidden" name="confirm" value="yes"/>
                <button>I asked for this link: sign in here</button>
            </form>
        </body></html>
    }
    if !r.ok { reply 401 r.reason }
    delete_cookie("sign_in_browser")
    set_session({ user: user_for(r.address) })
    redirect("/")
}

route POST /login/code { form: { email: str, code: str } } {
    let r = redeem({ address: email, code })
    if !r.ok { reply 401 r.reason }
    set_session({ user: user_for(r.address) })
    redirect("/")
}

every(600000, () => { run_sql(magic_links_sweep(MAGIC)?) })
```

The table, as `magic_links_table` writes it for SQLite (`sign_in_links` unless `table` says
otherwise):

```sql
create table if not exists sign_in_links (k varchar(64) primary key, a varchar(64) not null, address varchar(320) not null, code_hash varchar(64), expires bigint not null, tries integer not null, tried varchar(32), used varchar(32), browser varchar(64), seal varchar(64) not null);
create index if not exists sign_in_links_a on sign_in_links (a)
```

- **The token is after a `#`.** Browsers never send that part, so no request log, proxy or
  `Referer` holds it. The page reads it, and a button posts it: a mail scanner that opens
  every link (Outlook's Safe Links) uses nothing up, and signs no one in.
- **`tries` must be given.** Wrong codes before a code is spent: 5 is right. Without it,
  a script tries the million codes within their 15 minutes. `0`, written out, is no cap,
  for a program that counts guesses itself.
- **The link works in the browser that asked for it**, by the `sign_in_browser` cookie.
  Opened in another (a phone, for a link asked on a laptop), the visitor sees the address
  masked (`a•••@example.com`) and is asked to confirm, or to type the code: someone sent
  another person's link sees it is not theirs.
- **Limit the sends.** Each send is an email, and a code to guess at: count them per
  address and per client with [login_limits](#limits-on-guessing) (a limit of your own,
  `send: { tries: 3, per_seconds: 3600 }`), before `magic_links_start`.
- A new link or code ends the address's earlier ones, and the first sign-in ends the other
  half of its email. Links point at HTTPS only, or plain HTTP on `localhost` while
  developing.

## API keys

Keys for scripts, CI and partners, sent as `Authorization: Bearer <key>`. A key is made
once and shown once (`shop_live_` and 32 random characters with a check);
[api_keys](/plugins/api_keys/) keeps only its HMAC, with its owner, name, scopes and expiry,
each row sealed:

```quillo
use "api_keys.qwer"

db.open("app.db")
db.exec("create table if not exists orders (id integer primary key, user_id integer not null, total real not null)")
let KEYS = { secret: env("AUTH_SECRET"), prefix: "shop_live" }
db.exec(api_keys_table(KEYS)?)

fn run_sql(q: map) {
    let row = null
    for s in q.steps { row = db.one(s.sql, s.args)? }
    row
}

// The key a request carries, with `scope`: a missing or wrong key is answered 401 here, and a
// key without the scope 403, with the header RFC 6750 asks for.
fn key_for(req, scope: str) -> map {
    let header = req.headers.authorization ?? ""
    let found = api_keys_find(KEYS, header)
    let k = is_err(found) ? { ok: false, reason: error(found) } : api_keys_check(KEYS, header, run_sql(found)?)?
    if !k.ok { reply 401 respond(401, { error: k.reason }, { "www-authenticate": "Bearer error=\"invalid_token\"" }) }
    if !api_keys_allows(k.scopes, scope) {
        reply 403 respond(403, { error: `this key may not ${scope}` }, { "www-authenticate": `Bearer error="insufficient_scope", scope="${scope}"` })
    }
    k
}

route GET /api/orders {
    let k = key_for(request, "orders:read")
    db.query("select id, total from orders where user_id = ?", k.owner)
}

route POST /settings/keys { form: { name: str } } {
    let me = session().user ?? reply 401 "Sign in first"
    let made = api_keys_new(KEYS, me, { name, scopes: ["orders:read"], days: 90 })?
    run_sql(made)?
    <html><body><p>Your key, shown this once: <code>{made.key}</code></p></body></html>
}

route POST /settings/keys/revoke { form: { id: str } } {
    let me = session().user ?? reply 401 "Sign in first"
    let s = api_keys_revoke(KEYS, me, id)?.steps[0]
    db.exec(s.sql, s.args)
    redirect("/settings")
}
```

The table, from `api_keys_table` for SQLite:

```sql
create table if not exists api_keys (k varchar(64) primary key, id varchar(24) not null, owner varchar(255) not null, name varchar(100) not null, scopes text not null, made bigint not null, expires bigint not null, hint varchar(8) not null, seal varchar(64) not null);
create index if not exists api_keys_owner on api_keys (owner)
```

- A key is checked in constant time, and a row changed in the database (another owner, more
  scopes, a later expiry) fails its seal. A malformed key is refused before any SQL runs.
- `scopes` such as `"orders:read"`; `"orders:*"` covers the family, `"*"` all. Offer a list
  of your own and refuse others when a key is made.
- `api_keys_list` and `api_keys_listed` give a settings page each key's name, scopes,
  expiry and last four characters, never the key. Nothing records when a key was last
  used, which would be a write per request.
- Keys end after `days` (90; 0 never). Making a key removes the owner's keys that ended.
- For AI clients at the MCP door, Quillo has its own: a signed-in session, or a token
  `mcp_token()` makes for one (see [Tools for AI clients](#possible-or-not)).

## JWT, JWKS and PASETO

A token from an identity provider (Auth0, Okta, Entra ID, Cognito, Keycloak) is checked
against its keys, its audience and its issuer:

```quillo
use "jwt.qwer"

// A partner's token: its signature by the provider's keys, its expiry, and that it was made
// for this API by this provider. The route is answered 401 here otherwise.
fn partner(req) -> map {
    let token = replace(req.headers.authorization ?? "", "Bearer ", "")
    let claims = jwt_verify_jwks(token, {
        jwks_url: "https://id.example.com/.well-known/jwks.json",
        audience: "orders",
        issuer: "https://id.example.com/",
    })
    if is_err(claims) { reply 401 respond(401, { error: error(claims) }, { "www-authenticate": "Bearer" }) }
    claims
}

route GET /partner/stock = { for: partner(request).sub, stock: 12 }
```

- **Ask for the audience and the issuer.** A token must then have them: one without an
  `aud` or an `iss` is refused *(since jwt 3.0.0: it was taken, so a token the provider made
  for something else, or one this program signed for an unsubscribe link, passed)*.
- **Options are checked.** An option not known, or of the wrong type, is an error, never a
  check left undone: `{ audience: env("JWT_AUDIENCE") }` with the variable unset is an
  error, not "any audience" *(since jwt 3.0.0)*.
- `exp` is required unless `require: []` says otherwise; `nbf` is checked; the leeway is 30
  s, at most a day. The keys path takes RS256, ES256 and EdDSA, never HS256 or `none`, and
  fetches the keys again for a `kid` it does not have, at most every 30 s.
- `jwt_check(token, secret, { audience, issuer })` checks HS256 tokens this program signs
  with `jwt_sign`. `jwt_verify(token, secret)` checks no audience: use it only for a secret
  that signs nothing else. `jwt_peek` reads a token without checking it, for logs.
- The [jwks](/plugins/jwks/) plugin checks against a JWKS text the program fetched and
  keeps: `jwks_check(token, jwks, { audience, issuer })`. It refuses a token with no `exp`
  unless `allow_no_exp: true` *(since jwks 2.0.0)*.

[PASETO](/plugins/paseto/) v4 tokens have no algorithm to choose, so none to confuse:
`v4.local` is encrypted with a shared key, `v4.public` signed with Ed25519:

```quillo
use "paseto.qwer"

let ACCOUNTS_KEY = env("ACCOUNTS_PUBLIC_KEY", "")     // paseto_keypair().public of the service that signs

fn caller(req) -> map {
    let token = replace(req.headers.authorization ?? "", "Bearer ", "")
    let claims = paseto_verify_check(token, ACCOUNTS_KEY, { audience: "orders-api", issuer: "accounts" })
    if is_err(claims) { reply 401 { error: error(claims) } }
    claims
}

route GET /orders = { for: caller(request).sub }
```

A token with no `exp` (made with `expires_in` 0) is refused unless `allow_non_expiring: true`
*(since paseto 2.0.0: every check took one)*, and `paseto_decrypt_check` and
`paseto_verify_check` take the audience and issuer. [Biscuit](/plugins/biscuit/) tokens
carry their own rules, which a holder can narrow before passing them on.

Between Quillo services, core checks tokens itself: see [Calls between
services](#calls-between-services).

## HTTP Basic and Digest

The browser's own sign-in box, for an admin page, a staging site or a script. Basic sends
the name and password with every request, so it is refused unless the site is served over
HTTPS; [http_auth](/plugins/http_auth/) reads the header strictly, and the program checks
the password against its own hash:

```quillo
use "http_auth.qwer"

db.open("app.db")
db.exec("create table if not exists staff (name text primary key, password_hash text not null)")
let TLS = env("BEHIND_TLS", "") == "1"          // a proxy in front serves HTTPS

// The staff member a request's Basic credentials name; the route is answered 401 otherwise.
fn staff(req) -> str {
    let ask = http_basic_challenge("Shop admin", { tls: TLS })
    if is_err(ask) { reply 403 error(ask) }           // plain HTTP: no one is asked for a password
    let c = http_basic_parse(req.headers.authorization ?? "", { tls: TLS })
    let row = is_err(c) ? null : db.one("select password_hash from staff where name = ?", c.user)
    if is_err(c) || !crypto.verify_password(c.password, row.password_hash) {
        reply 401 respond(401, "Sign in", { "www-authenticate": ask })
    }
    c.user
}

route GET /admin = <html><body><p>Hello, {staff(request)}.</p></body></html>
```

```sh
curl -u "ada:correct horse" https://shop.example/admin
```

Digest never sends the password: the client proves it holds it, against a nonce the server
signed and dates. It is for devices and old clients. Keep `http_digest_ha1(name, realm,
password, "SHA-256")` for each user instead of the password, and as carefully: it opens the
realm as the password does.

```quillo
use "http_auth.qwer"

db.open("app.db")
db.exec("create table if not exists devices (name text primary key, ha1 text not null)")
let DIGEST = { secret: env("AUTH_SECRET", ""), realm: "Shop devices", algorithms: ["SHA-256"], minutes: 5 }

fn device(req) -> str {
    let config = { ...DIGEST, bind: req.ip }          // a nonce seen on the way is no use elsewhere
    let header = req.headers.authorization ?? ""
    let f = http_digest_parse(header)
    let row = is_err(f) ? null : db.one("select ha1 from devices where name = ?", f.username)
    let target = req.query_string == "" ? req.path : `${req.path}?${req.query_string}`
    let r = http_digest_check(config, header, req.method, target, row.ha1)?
    if !r.ok { reply 401 respond(401, r.reason, { "www-authenticate": http_digest_challenge(config, { stale: r.stale })? }) }
    r.user
}

route GET /device/status = { device: device(request), ok: true }
```

- A nonce lasts `minutes`; when only its time is past, the check says `stale`, and the
  challenge says so, so the client tries again without asking anyone.
- Offer MD5 only on a route of its own, for clients that know nothing else: given two
  challenges in one header, curl and Python pick the last.
- Both compare in constant time. A password is given as it came, not normalized.

## SAML

A company's identity provider (Entra ID, Okta, ADFS, Google Workspace) signs the visitor in
and posts a signed answer back. The [saml](/plugins/saml/) plugin is the service provider:
it reads the IdP's metadata, starts a sign-in, and checks the answer (the XML signature by
the IdP's certificate alone, the audience, the recipient, the time and the request it
answers), reading only what a signature covers, so signature wrapping fails. Two things are
the program's, and both need a table:

```quillo
use "saml.qwer"

db.open("app.db")
db.exec("create table if not exists saml_requests (relay text primary key, request_id text not null, made_at integer not null)")
db.exec("create table if not exists saml_used (assertion_id text primary key, until_ms integer not null)")
allow_cross_site("/saml/acs")                    // the IdP's page posts the answer from its site

let SP = { entity_id: "https://shop.example/saml", acs_url: "https://shop.example/saml/acs" }
let CONFIG = { ...SP, idp: saml_idp_metadata(read_file("idp-metadata.xml")?)? }   // saved from the IdP

route GET /saml/login {
    let relay = crypto.token(24)
    let req = saml_authn_request(CONFIG, { relay_state: relay })?
    db.exec("delete from saml_requests where made_at < ?", now() - 600000)
    db.exec("insert into saml_requests (relay, request_id, made_at) values (?, ?, ?)", relay, req.id, now())
    set_cookie("__Host-saml", relay, { same_site: "None", max_age: 600 })
    redirect(req.url)
}

route POST /saml/acs { form: { SAMLResponse: str, RelayState: str = "" } } {
    let held = cookie("__Host-saml") ?? ""
    delete_cookie("__Host-saml", { same_site: "None" })
    if held == "" || !crypto.equal(held, RelayState) { reply 401 "This sign-in was not started in this browser" }
    let row = db.one("select request_id from saml_requests where relay = ? and made_at > ?", RelayState, now() - 600000)
    if row == null || db.exec("delete from saml_requests where relay = ?", RelayState).changes != 1 { reply 401 "That sign-in was used already" }
    let r = saml_read_response(CONFIG, SAMLResponse, { request_id: row.request_id, allow_replay: true })
    if is_err(r) {
        print(`SAML sign-in refused: ${error(r)}`)
        reply 401 "The sign-in did not work: try again."
    }
    db.exec("delete from saml_used where until_ms < ?", now())
    let kept = db.exec("insert into saml_used (assertion_id, until_ms) values (?, ?) on conflict (assertion_id) do nothing", r.assertion_id, date.ms(r.not_on_or_after) + 120000)
    if kept.changes != 1 { reply 401 "That answer was used already" }
    set_session({ user: r.name_id })
    redirect("/")
}

route GET /saml/metadata = respond(200, saml_metadata(SP)?, { "content-type": "application/samlmetadata+xml" })
```

- **The request id waits in a table, found by the RelayState.** The IdP's form post comes
  from another site, so it carries no `SameSite=Lax` session cookie: a request id kept in the
  session is gone at the ACS. The RelayState comes back with the answer, and a
  `SameSite=None` cookie of its own does come with such a post, so the ACS takes an answer
  only from the browser that started that sign-in. Without the request id, only
  `allow_unsolicited: true` gets through, and anyone could then sign a visitor in as
  themselves.
- **Assertions used are kept in a table** with a unique key, until they would end anyway.
  The plugin's own record is the process's: a restart, or a second server, would take the
  same answer again. `allow_replay: true` says the table is the record.
- DTDs are refused, SHA-1 signatures too unless `allow_sha1: true`, and so are encrypted
  assertions (ADFS and some Entra setups send them by default: turn encryption off for this
  application). There is no single logout.
- `saml_idp_metadata` does not check the metadata's own signature: save it from the IdP's
  HTTPS address, or from its admin.

## LDAP and Active Directory

An intranet's sign-in against the directory, with [ldap](/plugins/ldap/): the person is
found by a search as the service account, then checked by binding as that entry on a
connection of its own, over LDAPS or StartTLS:

```quillo
use "ldap.qwer"

let BASE = env("LDAP_BASE", "DC=corp,DC=example")
let USERS = "(&(objectClass=user)(sAMAccountName={user}))"      // OpenLDAP: (uid={user})
let URL = env("LDAP_URL", "ldaps://dc1.corp.example")

fn connect() {
    let d = ldap_connect("ad", URL, {
        bind_dn: env("LDAP_BIND_DN", ""),
        password: env("LDAP_PASSWORD", ""),
        starttls: starts_with(URL, "ldap://"),      // ldaps:// is TLS from the start
        ca: env("LDAP_CA", ""),                     // the CA's PEM, for a certificate of the company's own
        timeout_ms: 5000,
    })
    if is_err(d) { print(`the directory: ${error(d)}`) }
    d
}

// A directory not reached at start is tried again each minute; a connection dropped later is
// opened again by the plugin itself.
let directory = connect()
every(60000, () => { if is_err(directory) { directory = connect() } })

route POST /sign-in { form: { user: str, password: str } } {
    let started = now()
    let r = ldap_sign_in("ad", BASE, USERS, trim(user), password, { attrs: ["mail", "displayName"] })
    if !is_err(r) && r.ok {
        set_session({ user: r.dn })
        return redirect("/")
    }
    // The reason (no such user, locked out, password expired) is for the log, never the visitor.
    print(`sign-in refused for ${json.stringify(slice(user, 0, 64))}: ${is_err(r) ? error(r) : r.reason}`)
    sleep(max([0, 400 - (now() - started)]))          // an unknown name answers as late as a known one
    reply 401 "Wrong user name or password"
}
```

- **One answer, at one time, for every failure.** An unknown name costs a search, a known
  one a search and a bind, so the wait hides which accounts exist; the reasons stay in the
  log.
- **Limit guesses below the directory's lockout.** Active Directory locks an account after
  a few wrong passwords, so anyone could lock out your staff from the sign-in page. Count
  tries per address and per account with [login_limits](#limits-on-guessing), the account
  keyed by the entry a search finds (the plugin's example does), under AD's threshold.
- Take the password from a form posted, never a query string, which proxies and the
  browser's history keep; Quillo's own request log writes a `password` there `[hidden]`
  *(since 0.8.9: it printed it)*.
- `{user}` is escaped for the filter (`ad*` stays three letters), an empty password is
  refused before any bind, and a name that matches two entries signs no one in.
  `ldap_groups(name, dn, base, { nested: true })` gives a user's groups, nested ones by
  AD's matching rule.

## RADIUS

Wi-Fi portals, VPNs and network gear often check people against a RADIUS server.
[radius](/plugins/radius/) asks one, with PAP or CHAP:

```quillo
use "radius.qwer"

route POST /portal { form: { user: str, password: str } } {
    let r = radius_auth(env("RADIUS_SERVER", "10.0.0.9"), env("RADIUS_SECRET", ""), user, password, { nas_identifier: "portal" })
    if is_err(r) || !r.accepted { reply 401 "Wrong user name or password" }
    set_session({ user }, { max_age: r.attributes["Session-Timeout"] ?? 3600 })
    redirect("/")
}
```

- Every request carries a Message-Authenticator, and an answer without one is refused
  *(since radius 2.0.0: it was taken unless asked)*: BlastRADIUS forges an accept from a
  reject where answers have none. A server older than 2024 needs
  `require_message_authenticator: false`, here and in `radius_status`.
- A server that answers with a challenge (a one-time code) gives `challenge: true` and a
  `state`: ask the visitor, then call again with `{ state: r.state }`.
- RADIUS is for a network you trust: there is no RadSec (RADIUS over TLS) here. Use a long
  shared secret, and limit guesses as for passwords.
- `radius_parse`, `radius_password_ok` and `radius_reply` make a RADIUS server of your own
  with `udp.listen`.

## Calls between services

Quillo services sign their calls themselves. By default every service holds
`QUILLO_SERVICE_SECRET`, which proves only that the caller is one of the system's. A caller
can prove more, and a service can say whom it takes calls from:

```quillo
// shop.qwe
service billing = "billing.qwe" { auth: { key: env("SHOP_KEY"), as: "shop" } }

route POST /checkout { json: { cents: int } } = billing.refund(42, cents)
```

```quillo
// billing.qwe
accept_calls {
    keys: { shop: env("SHOP_KEY") },
    jwt: { issuer: env("IDP"), audience: "billing", jwks: env("IDP") + "/.well-known/jwks.json" },
}

server fn refund(account: int, cents: int) for ["shop"] {
    { refunded: cents, by: request.caller.name }
}
```

- `keys: { shop: ... }` gives each caller a key of its own, so a service that leaks its key
  cannot call as another. `for ["shop"]` takes calls from those callers only; pages cannot
  call it at all.
- `jwt` takes OAuth 2.0 access tokens from an identity provider, for callers that are not
  Quillo services. Its check is core's: `audience` and `exp` required, `iss` and `nbf`
  checked, `alg: none` and HS256 keyed with the public key refused. The token must be the
  caller's own: a user's token from Okta or Entra ID v1, whose client is in `cid` or
  `appid`, is refused *(since 0.8.9: it passed as the service's own)*.
- `client_cert` takes the identity of a certificate a proxy checked (Envoy's
  `x-forwarded-client-cert`, a SPIFFE id), believed only from an address `--trust-proxy`
  names.

[Authentication between services](#microservices-authentication-between-services) has
each option, rotating keys, and signed answers.

## Client certificates and Kerberos

Quillo ends no TLS itself, and speaks no Kerberos. Both are done by a proxy in front, which
tells the program who the visitor is in a header. Name that header when the program starts,
with the proxy, and the routes read it as `request.client_cert` and `request.remote_user`
*(since 0.8.9: the program read the header itself)*:

```sh
quillo run main.qwe --trust-proxy 127.0.0.1 \
  --client-cert-header x-client-dn --client-cert-subject --user-header x-remote-user
```

- **`--client-cert-header NAME`**: the header a proxy that ended mutual TLS sets.
  `request.client_cert` is then `{ name, subject, uri }`: `name` the certificate's SPIFFE id
  where it has one, else its subject's CN; `subject` the subject as the proxy wrote it; `uri`
  its URI, where it has one. The header is read as `accept_calls { client_cert }` reads it
  for calls between services: Envoy's `x-forwarded-client-cert`, one proxy's element, unless
  `--client-cert-subject` says it holds the subject alone, in RFC 2253's form (nginx's
  `$ssl_client_s_dn`).
- **`--user-header NAME`**: the header a proxy that signed the person in sets (Kerberos by
  SPNEGO: Apache's mod_auth_gssapi, IIS). `request.remote_user` is the user as the proxy wrote
  it, trimmed (`ada@CORP.EXAMPLE`, `CORP\ada`).
- **Believed from the proxy alone.** Each is read only on a request from an address
  `--trust-proxy` names, and either option stops the program at start without it (`quillo:
  --client-cert-header and --user-header read a proxy's header, believed only from the
  addresses --trust-proxy names: give --trust-proxy too (any visitor can send that
  header)`). The proxy must set the header in place of any the visitor sent: each field is
  null where its header is missing, came from another address, or is not one value the proxy
  set (several elements of a certificate's header, several lines, or, for `--user-header`, a
  value with a comma in it, as a proxy that appends puts the visitor's first; a subject's own
  commas, `CN=Ada,OU=Staff`, are its parts). The log says why, once. Keep Quillo
  where only the proxy reaches it: on its default `--host`, 127.0.0.1. On a machine others
  share, `--listen unix:app.sock --socket-mode 660 --trust-proxy 127.0.0.1` keeps out other
  users' programs too.
- Where neither option names a header, `request` has neither field. A service `--split`
  starts gets both from its app, signed.

nginx, asking for a certificate from the company's CA:

```nginx
ssl_client_certificate /etc/nginx/staff-ca.pem;
ssl_verify_client on;
location / {
    proxy_set_header X-Client-DN $ssl_client_s_dn;    # replaces any the visitor sent
    proxy_pass http://127.0.0.1:8080;
}
```

Apache with mod_auth_gssapi, for Kerberos (SPNEGO, Windows' integrated sign-in):

```apache
<Location />
    AuthType GSSAPI
    AuthName "Corp sign-in"
    GssapiCredStore keytab:/etc/apache2/http.keytab
    Require valid-user
    RequestHeader set X-Remote-User expr=%{REMOTE_USER}
</Location>
ProxyPass / http://127.0.0.1:8080/
```

The routes then read the fields, and compare them whole with what the program's table holds:

```quillo
db.open("app.db")
db.exec("create table if not exists staff (id integer primary key, cert_dn text unique, principal text unique)")

route GET /staff {
    let cert = request.client_cert ?? reply 401 "A staff certificate is needed"
    let row = db.one("select id from staff where cert_dn = ?", cert.subject) ?? reply 403 "Not on the staff list"
    <html><body><p>Staff member {row.id}: {cert.name}</p></body></html>
}

route GET /intranet {
    let who = request.remote_user ?? reply 401 "Sign in to Windows first"      // ada@CORP.EXAMPLE
    let row = db.one("select id from staff where principal = ?", lower(who)) ?? reply 403 "Not on the staff list"
    <html><body><p>Hello, {who}: staff member {row.id}</p></body></html>
}
```

Compare the whole subject, or `name`, rather than picking a CN out of the subject by hand: a
subject's escapes (`CN=Acme\, Inc.`) make a hand-written parser easy to fool. Before 0.8.9,
read the header yourself (`request.headers["x-client-dn"]`), as this guide showed then.

## An OAuth authorization server

Quillo cannot yet be the server that gives other apps tokens to act for its users (the
authorization code with PKCE, refresh tokens, the device flow): it is planned, as a plugin.
Until then:

- for scripts and partners, give out [API keys](#api-keys);
- for AI clients, the MCP door takes a signed-in session or an `mcp_token()`;
- where other systems need OAuth tokens, run an identity provider (Keycloak, Authentik,
  Zitadel, or a hosted one), sign your users in through it with [oidc](#sign-in-with-google-github-microsoft-or-apple),
  and check its tokens with [jwt](#jwt-jwks-and-paseto) or `oidc_verify_access`.

## Upgrading the auth plugins

The catalog's auth plugins were reviewed in November 2026, and those whose fixes change
what a program sees moved to a new major version. `quillo install --update` brings them; a
file you changed by hand is kept unless `--force`. Each changed function's `///` line, on
its plugin page, says what to change:

| Plugin | What changed |
|---|---|
| [jwt](/plugins/jwt/) 3.0.0 (from 2.x) | A token with no `aud` or `iss` is refused when `audience` or `issuer` is asked for, in `jwt_check`, `jwt_verify_jwks` and `jwt_verify_rs256`: have such tokens name them, or stop asking. An option not known, or of the wrong type (a null from an unset variable), is an error. `audience` and `issuer` take a list. New: `jwt_sign_es256` |
| [jwks](/plugins/jwks/) 2.0.0 (from 1.x) | A token with no `exp` is refused: `jwks_check(token, jwks, { allow_no_exp: true })` takes one. `nbf` is checked, and `aud` and `iss` must be there when asked for. `jwks_check`'s options are checked |
| [paseto](/plugins/paseto/) 2.0.0 (from 1.x) | A token with no `exp` is refused: `paseto_decrypt_check` and `paseto_verify_check` with `allow_non_expiring: true` take one, and an `audience` and `issuer` |
| [mfa_codes](/plugins/mfa_codes/) 2.0.0 (from 1.x) | Hashes are salted Argon2id: check with `mfa_check`, never a `where hash = ?` lookup, in a column of about 100 characters. 1.x's hashes still verify; `mfa_legacy` finds them, so a set can be replaced at its next use. A check costs about 25 ms per stored hash |
| [passkeys](/plugins/passkeys/) 2.0.0 (from 1.x) | User verification is required unless `require_user_verification: false`, and the options ask for it. Attestation by a certificate is `unverified-certificate`, not `basic` |
| [bcrypt](/plugins/bcrypt/) 2.0.0 (from 1.x) | `bcrypt_hash` refuses a password over 72 bytes: hash those with `crypto.hash_password` |
| [radius](/plugins/radius/) 2.0.0 (from 1.x) | An answer without a Message-Authenticator is refused: a server older than 2024 needs `require_message_authenticator: false` |
| [totp](/plugins/totp/) 1.1.0 | New: `totp_check`, which gives the time step, so a code is taken once. `totp_verify` is unchanged |
| [oidc](/plugins/oidc/) 1.2.0 | Microsoft's `common` and `organizations` endpoints work, with `tenants` or `any_tenant: true`. Options are checked: one not known, or of the wrong type, is an error |

New in the catalog: [login_limits](/plugins/login_limits/), [magic_links](/plugins/magic_links/),
[api_keys](/plugins/api_keys/) and [http_auth](/plugins/http_auth/). The examples of
[ldap](/plugins/ldap/), [saml](/plugins/saml/) and [oauth_pkce](/plugins/oauth_pkce/) were
written again: the LDAP sign-in posts a form and answers one way for every failure, SAML
keeps its request ids and used assertions in tables, and GitHub's token stays on the server.
