# Feedback from studying authentication protocols in Quillo: what exists, what works, how safe it is, and a built-in layer

| | |
|---|---|
| App | None: a study at the user's request of 2026-11-19, "we need to implement all common authentication protocols". After seeing what exists, the user chose "Study first, then build": this report is to choose from, and building waits for the FEEDBACK75 batch |
| Quillo | `quillo 0.8.8 (f35bf2d, 2026-11-19)`, the release on the PATH, for every probe. The checkout (branch `feedback75`, where five agents are fixing FEEDBACK75) was read, not built |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | The 23 auth plugins of the catalog, copied into `.git/wip/feedback78/plugins/` and built there in one cargo call, queued behind the shared cargo lock. Each was run against its catalog checks, with the catalog's own stand-ins: a test OIDC issuer, a SAML IdP, an LDAP directory, a RADIUS server, a software authenticator and a fake YubiCloud. One probe program for each finding. Core's `accept_calls { jwt }` was tested against a JWKS served on loopback, with 12 tokens made in Python: signed, forged, `alg: none`, and HS256 keyed with the public key. Headless Chrome, behind `.git/wip/feedback75/chrome.lock`, ran passkeys through `js()` with a CDP virtual authenticator, and a form on another site posting to an ACS. curl, and a Python loop for guesses. Microsoft's, Apple's and Google's public discovery documents, read without any account. Read: `src/crypto.rs`, `src/svcauth.rs`, `src/session.rs`, `src/push.rs`, `src/mcp.rs` (tokens), `src/db.rs` (Quillo's own tables), `src/client/runtime.js`, `src/error.rs`, the guide, and the source of every auth plugin |

Nothing was changed in Quillo's source, `feedback/INDEX.md`, `feedback/repros/` or `~/dev/quillo-website`. This report took the block Q-1150 to Q-1179 and uses Q-1150 to Q-1167; FEEDBACK76.md and FEEDBACK77.md hold Q-1130 to Q-1149. The repros are written out below, and the probes are in `.git/wip/feedback78/`. Under the rule of 2026-11-05, nothing here is built until main is released and in sync, and the user put the FEEDBACK75 batch first.

**A correction to the brief.** Core has no TOTP. `crypto.totp_secret`, `crypto.totp_uri`, `crypto.totp_verify`, `crypto.recovery_codes` and `crypto.verify_recovery_code` were removed in 0.5.0 (205d7cd). On 0.8.8, `print(crypto.totp_secret())` gives ``error: `crypto` has no function `totp_secret`: Quillo removed it in 0.5.0 (205d7cd); write it in Rust, in a .qwer file the program uses`` (src/error.rs:165). TOTP and recovery codes exist only as the `totp` and `mfa_codes` plugins.

**How the request is read.** "All common authentication protocols" is read as three groups:

- **The everyday set**, which most web apps need: passwords, sessions, "Sign in with Google, Microsoft, Apple or GitHub", passkeys, two-factor codes with recovery codes, sign-in by email link or code, and API keys.
- **Tokens between programs**: JWT, JWKS, PASETO, bearer tokens, calls between services, client certificates, and an authorization server that gives tokens to other apps.
- **The enterprise set**: SAML, LDAP and Active Directory, RADIUS and Kerberos.

"Implement" is read as: a program gets each one right in a few lines, and the dangerous parts (signatures, single use, limits on guessing, storage) are done by Quillo or by an audited plugin.

What worked well:

- **The catalog already holds most of the protocols, and they work.** All 23 auth plugins pass their catalog checks on 0.8.8, against stand-in servers.
- **The hard plugins are careful.**
  - SAML refuses every signature-wrapping variant its test IdP sends. It reads the NameID across comments, refuses DTDs and SHA-1, and reads only what a signature covers. One parser does both the checking and the reading.
  - OIDC checks state, nonce, PKCE, `iss`, `aud`, `azp` and `at_hash`, and takes only RS256 and ES256.
  - LDAP escapes its filters, and refuses an empty password before it binds.
- **Core's own token check is strict.** `accept_calls { jwt }` refused all 11 bad tokens sent to it, `alg: none` and HS256 keyed with the public key among them.
- **Core's password hashing is sound.** It is Argon2id at OWASP's floor, run off the event loop, one hash per core. A stand-in hash makes an unknown email take as long as a known one.
- **Passkeys work today**, through the plugin and `js()`, in a component with no script of the program's own, once the credential goes back through `JSON.stringify` (Q-1159).

## The answer

**Build a small `auth` layer in core for the everyday set, and keep the enterprise protocols as plugins, with the fixes below.**

- **Most of the code exists.** The OIDC, passkeys, TOTP and recovery-code plugins can move in nearly as they are, on crates core already links (`ring`, `p256`, `ureq`, `argon2`).
- **What the layer adds is what no plugin can do, and that is where the measured failures were:**
  - a record of what was used once, shared by every server;
  - a limit on guessing;
  - sessions that can be ended on the server;
  - flows that do not need the session cookie when the provider sends the visitor back from another site;
  - the browser half of passkeys.

| Protocol | Today | Works on 0.8.8 | Security found | Recommendation |
|---|---|---|---|---|
| Passwords: hashing, migration, strength, breaches | Core: Argon2id. Plugins: argon2, bcrypt, scrypt, legacy_passwords, password_strength, pwned, passphrase | Yes (probed) | Core sound. bcrypt reads 72 bytes (Q-1165). Core never says when a hash should be made again | Built-in: a check that reads Argon2 and bcrypt and says `rehash`. The rest stays plugins, with a guide recipe |
| Sessions, remember-me | Core: signed cookies, `max_age` | Yes | A copied cookie outlives sign-out (Q-1157) | Built-in: end a user's sessions on the server |
| OAuth 2.0 / OIDC sign-in | Plugins: oidc, oauth_pkce | Yes against a test issuer. Not with Microsoft's multi-tenant endpoints (Q-1160) or Apple (Q-1161) | oidc sound. A sample keeps a token in the cookie (Q-1167) | Built-in: move oidc in, with presets per provider |
| Passkeys (WebAuthn) | Plugin: passkeys (server half) | Yes, through `js()` and a JSON round trip (Q-1159) | Sound. User verification is not required by default (Q-1163) | Built-in, with the browser half in the runtime |
| TOTP, recovery codes, hardware OTP | Plugins: totp, mfa_codes, yubico_otp | Yes | Codes reused (Q-1152), recovery hashes unsalted (Q-1153), no limit on guesses (Q-1151) | Built-in for TOTP and recovery codes. YubiKey OTP stays a plugin |
| Magic links, email and SMS codes | Parts only: `crypto.token`, smtp, mail_api, sms | A recipe works | Tokens in URLs reach the log (Q-1158) | Built-in links and codes. Sending stays the program's |
| API keys, bearer tokens | `mcp_token()`, for the MCP door only | Not for routes | — | Built-in: widen `mcp_token` to routes |
| JWT, JWKS, PASETO verification | Core: `verify_jwt`, for services only. Plugins: jwt, jwks, paseto, biscuit | Yes | Core: 11 bad tokens of 11 refused. Plugins: Q-1150, Q-1154, Q-1164 | Built-in: open core's checker to routes. Signing, PASETO and Biscuit stay plugins |
| HTTP Basic, Digest | Nothing: core cannot decode base64 | Basic: with the encoding plugin, or a precomputed header | — | A recipe for Basic. Not Digest |
| SAML 2.0 service provider | Plugin: saml | Yes, with the test IdP | Careful. Replays are remembered per process (Q-1156). The ACS gets no session (Q-1155) | Plugin, audited, with fixes |
| LDAP, Active Directory | Plugin: ldap | Yes, with the stand-in | Escaping and the empty-password guard are sound. The sample enumerates accounts (Q-1162) | Plugin |
| RADIUS | Plugin: radius | Yes, with the stand-in | Message-Authenticator not required by default (Q-1166) | Plugin |
| Kerberos / SPNEGO | Nothing | — | — | A recipe: a proxy does SPNEGO, and core reads its header |
| mTLS, client certificates | Services only (`client_cert`, behind a trusted proxy) | Services: yes. Routes: no | Services sound since 0.8.7 | Built-in `request.client_cert` for routes, with the same parser |
| OAuth 2.0 authorization server | Nothing | — | — | Later, built-in and narrow (decision 6) |
| Service-to-service | Core: `auth` and `accept_calls` | Yes (probed) | Sound | As it is |
| Limits on guessing (for all of the above) | None for routes. The rate_limit plugin counts per process | — | One connection: 19,882 guesses a second (Q-1151) | Built-in, kept in the database |

The four impacts of the built-in layer:

| Impact | An `auth` layer in core for the everyday set |
|---|---|
| **Functionality** | Each everyday protocol takes a few lines. A password check reads bcrypt and says when to rehash. Sign-in with Google, Microsoft (any tenant), Apple, GitHub or any OIDC issuer. Passkeys from a button. TOTP and recovery codes, each used once. Email links and codes. API keys that make `session().user` the key's user. JWT checks with core's rules. No change for a program that uses none of it |
| **User** | Visitors: passkeys and "Sign in with..." that work on every server, codes that cannot be replayed, and a sign-out that ends a stolen copy of the cookie. Developers write no JavaScript, keep no flow state in cookies, design no tables and write no limits, and get the same `session().user` whether the caller used a cookie, a passkey or a key |
| **Requests** | Nothing for a program without `auth`. With it: one database round trip per attempt for the limit and for single use (one upsert). One or two calls out per OIDC sign-in (the token, and the provider's keys about once an hour). Two POSTs per passkey ceremony. One cached read per request with a session, only where sessions are ended on the server |
| **Security** | Guessing is limited by default in every check. Codes, challenges, states, links and SAML assertions are used once, recorded in the database, across restarts and servers. Recovery codes and keys are stored as HMACs under a derived key, and TOTP secrets are encrypted. Sessions can be ended on the server. New stored data: factors, keys, flows and limits, in Quillo's own tables, each tied to `session().user` and removed with the user. Every fix to these protocols needs a Quillo release |

What today's way gives that the built-in layer would lose:

- **Tables of the program's own.** A program that keeps TOTP secrets in its own `users` table cannot do that with the built-in. It can still use the plugins.
- **A small core.** Core grows by an estimated 2,500 to 3,500 lines with tests (an estimate, not counted), most of it moved plugin code.
- **Updates on their own schedule.** A provider's change (Apple's, Microsoft's) needs a Quillo release. A plugin updates alone.
- **Choice.** The built-in has one way to do each thing.

**My recommendation:** build it in two phases (see "Proposed order of work"):

1. **Phase 1:** the limit, the single-use record, passwords with `rehash`, TOTP with recovery codes, and ending sessions on the server.
2. **Phase 2:** OIDC providers, passkeys, email links, API keys and the JWT check.

An authorization server would come after, if wanted (decision 6). SAML, LDAP and RADIUS stay plugins, with Q-1155, Q-1156, Q-1162 and Q-1166 fixed. The plugin fixes can ship before any of this, as plugin releases.

## Part A: what exists and what works, protocol by protocol

| Check | Result |
|---|---|
| The 23 auth plugins' catalog checks (`test.mjs`), on 0.8.8, with their stand-ins | 23 passed, 0 failed: argon2, bcrypt, scrypt, legacy_passwords, password_strength, pwned, passphrase, oauth_pkce, oidc, jwt, jwks, paseto, biscuit, passkeys, saml, ldap, radius, totp, mfa_codes, yubico_otp, rate_limit, csrf_tokens, cookies. Built from scratch in 80 s on cached crates |
| `crypto.totp_secret()` | Error: removed in 0.5.0 (205d7cd) |
| `crypto.hash_password` and `crypto.verify_password` | `$argon2id$v=19$m=19456,t=2,p=1$...`. Right password: true. Wrong one, a bcrypt hash, null, and an empty password: false. 0.14 s for the whole script |
| `accept_calls { jwt }`, 12 tokens | 1 good token taken. 11 refused, each with its reason (Part B) |
| A session cookie copied before `/logout` | Still signed in after it (Q-1157) |
| `jwt_check(t, key, { audience, issuer })` on a token with neither | Taken (Q-1150). `jwt_verify_jwks` the same |
| `jwks_verify` on a token with no `exp`, `aud` or `iss` | Taken (Q-1154) |
| `paseto_decrypt` on a token made never to expire, for another audience | Taken (Q-1164) |
| `totp_verify` on codes from the steps −3 to +3 | −1, 0 and +1 taken, the rest refused. The same code twice: true, true (Q-1152) |
| Guesses at a TOTP route over one kept connection | 19,882 a second. One was right within 3 s (Q-1151) |
| `mfa_hash` of one code, written two ways | The same hash: there is no salt (Q-1153) |
| One SAML response posted 8 times to `--instances 1`, then `--instances 4` | Signed in once each time, 7 refused as replays |
| The same response after a restart | Signed in again (Q-1156) |
| A form on 127.0.0.1 posting to localhost's `/acs`, after localhost set the session | `session at the ACS: none`. Sent there by a GET redirect: the session was there (Q-1155) |
| Passkeys in Chrome through `js()`, the credential passed to a `server fn` as it is | `the credential's type is null` (Q-1159) |
| The same, passed as `json.parse(js("JSON.stringify", cred))?` | Registered (ES256, user verified) and signed in (counter 2) |
| `oidc_discover` on an issuer whose metadata says `.../{tenantid}/v2.0` | Refused (Q-1160). Microsoft's `common` and `organizations` say exactly that |
| The ldap sample's `/sign-in`, for a wrong password, an unknown name, a locked account, `ad*` and an injection | "wrong password" with the DN, "no such user", "account locked out", and "no such user" for both of the last two. 3.5 ms against 0.5 ms on the server. The password printed in the request log (Q-1158, Q-1162) |
| bcrypt: a 90-byte password hashed, then another that shares its first 72 bytes | Verifies (Q-1165) |

### A.1 Passwords

- **Today, in core:** `crypto.hash_password` (Argon2id, m=19456, t=2, p=1, salted) and `crypto.verify_password`. The check is false for anything that is not an Argon2 hash, and takes as long for a missing hash as for a real one (a stand-in hash, src/crypto.rs). It runs off the event loop, at most one hash per core, with 1,024 waiting at most.
- **Today, in the catalog:**
  - argon2: tuned costs, `argon2_needs_rehash`, and `argon2_tune`, which measures this server.
  - bcrypt: checks `$2a$`, `$2b$` and `$2y$` hashes.
  - scrypt: PHC strings, Node's `crypto.scrypt`, and Firebase's modified scrypt.
  - legacy_passwords: Django's PBKDF2, Werkzeug, WordPress phpass and WordPress 6.8's `$wp$` bcrypt. Rounds are capped at 5 million, so a hash planted in the database cannot hold a core.
  - password_strength (zxcvbn), pwned (Have I Been Pwned by k-anonymity: 5 hex characters leave the server) and passphrase (the EFF lists).
- **Works:** yes, all of them (probes and catalog checks).
- **Security:** core is sound. bcrypt reads 72 bytes and says nothing (Q-1165). Core's cost is fixed and it never says a hash is due to be made again. So moving users off bcrypt is two plugins and a branch the program writes.
- **Recommendation:** built-in `auth.password(pw, stored, { limit })`, giving `{ ok, rehash, wait }`. It reads Argon2 with any cost, and bcrypt, which most imports are. Scrypt, Firebase, Django and WordPress stay plugins, with a guide recipe: verify the old hash, then rehash on success. Strength and breach checks stay plugins, with a recipe: zxcvbn's word lists are large, and Have I Been Pwned is a call out that the program chooses to make.

### A.2 Sessions and remember-me

- **Today:** a signed cookie (HMAC-SHA256 under `QUILLO_SECRET`), `__Host-quillo_session`, HttpOnly, Secure, SameSite=Lax. It lasts 30 days, or `max_age`. A hidden id (`"i"`) is carried across `set_session` calls; MCP tokens and push subscriptions are bound to it. `end_session()` removes the cookie and ends that session's MCP tokens and subscriptions. Remember-me is `max_age`.
- **Works:** yes. The probe's cookie payload decodes to `{"e":1797682511,"d":{"user":42},"a":1,"i":"2b9997ad50a4554aefd6c73140b0c9e0"}`.
- **Security:**
  - A copy of the cookie works after sign-out until it expires (Q-1157). The guide says so.
  - There is no list of a user's sessions ("where am I signed in").
  - The payload is readable, so it must hold no secrets (Q-1167).
  - Login CSRF is blocked: another site cannot post a form here.
  - The cookie cannot be planted from a sibling subdomain, because of the `__Host-` prefix.
- **Recommendation:** built-in: `end_session({ everywhere: true })`, also run when a password changes. It costs nothing for a program that never calls it (decision 3). Signing in should also give the session a new hidden id, as it is carried from before sign-in today.

### A.3 OAuth 2.0 and OpenID Connect sign-in

- **Today, two plugins:**
  - oidc 1.1.0: discovery, PKCE (S256, always), state, nonce, the code exchange (Basic or form), and the ID token checked. Only RS256 and ES256, then `iss`, `aud`, `azp`, `exp`, `iat`, `nbf`, `nonce`, `at_hash` and `max_age`. Also userinfo (its `sub` checked), refresh, client credentials, RFC 9068 access tokens and RFC 9207's `iss`.
  - oauth_pkce 1.0.0: links and token requests for Google, GitHub and Microsoft. It has no ID token check.
- **Works:** oidc's 9 catalog checks pass against the stand-in issuer: the flow, rotated keys, and 13 refusals (expired, other audience, other issuer, wrong nonce, forged, `alg: none`, HS256, wrong `at_hash`, other `sub`, denied, stolen state, wrong verifier, second use of a code).
- **Per provider:**
  - **Google:** works as plain OIDC. The issuer is `https://accounts.google.com`, and S256 is listed.
  - **Microsoft:** one tenant only. The `common` and `organizations` issuer is the template `https://login.microsoftonline.com/{tenantid}/v2.0`, so discovery is refused (Q-1160).
  - **Apple:** its client secret is an ES256 JWT that no plugin makes. When the name or email is asked for, it returns by `form_post`, where the session cookie is not sent (Q-1161, Q-1155).
  - **GitHub:** not OIDC. oauth_pkce gets the token. The program must then read `/user/emails` and use only verified addresses: linking an account by an unverified email is a takeover.
  - **Any other OIDC issuer:** works.
- **Security:** oidc is sound. It compares the state with `!=`, not in constant time. That leaks nothing in practice, since the value is random and the visitor's own. The oauth_pkce sample keeps the access token in the cookie, which anyone holding the cookie can read (Q-1167).
- **Recommendation:** built-in, by moving oidc in, with:
  - presets for Google, Microsoft, Apple and GitHub;
  - the flow's state kept in a table, keyed by `state`;
  - a short `__Host-` flow cookie with `SameSite=None`, so a `form_post` return still finds the flow (Part C).

### A.4 Passkeys (WebAuthn)

- **Today:** plugin passkeys 1.0.0, the server half. It makes the options, checks registration (`none` and `packed` attestation) and sign-in, the counter, the BE and BS flags, and ES256, EdDSA and RS256 signatures, all by hand. The browser half is the program's: the plugin's sample shows it in JavaScript.
- **Works:** yes, in headless Chrome with a virtual authenticator, from a component:
  - `js("PublicKeyCredential.parseCreationOptionsFromJSON", o)` and `js("navigator.credentials.create", { publicKey: opts })` ran.
  - The credential reached the server fn empty, until it was passed as `json.parse(js("JSON.stringify", cred))?` (Q-1159).
- **Security:**
  - Checked: the exact origin list, the rpIdHash, the type, and user presence. A counter that does not rise is refused, and so is BS without BE.
  - User verification is checked only when asked for (Q-1163).
  - `packed` attestation with a certificate is reported as `basic`, with no chain checked. The doc says the chain is the caller's to judge.
  - Challenges kept in the session cannot be "used once". A copy of the cookie keeps the challenge. Synced passkeys count 0, so the counter does not stop a captured (response, cookie) pair from being replayed (read, not run).
- **Recommendation:** built-in. The browser half goes in the runtime's private registry (`passkey.add()`, `passkey.sign_in()`), and challenges go in the single-use record.

### A.5 TOTP, recovery codes and hardware OTP

- **Today, three plugins:**
  - totp 1.0.0: totp-rs, SHA-1, 6 digits, 30 s, one step either side.
  - mfa_codes 1.0.0: codes of 10 characters (49.5 bits), SHA-256 with a fixed prefix, compared in constant time.
  - yubico_otp 1.0.0: YubiCloud with a nonce and signed answers, or self-hosted AES with its counter checked.

  FIDO2 security keys are passkeys (A.4).
- **Works:** the catalog checks pass. Codes from steps −1, 0 and +1 are taken, and −2 and +2 refused. The same code was taken twice.
- **Security:**
  - A code can be reused (Q-1152).
  - Nothing limits guessing: 19,882 guesses a second, and one hit within 3 s (Q-1151).
  - Recovery codes are hashed without a salt (Q-1153).
  - `yubico_verify` takes any valid YubiKey. Tying the key's public id to the user is the program's job.
- **Recommendation:** TOTP and recovery codes built in, about 150 lines on `ring`'s HMAC-SHA1 and HMAC-SHA256, both already linked. YubiKey OTP stays a plugin.

### A.6 Magic links, and codes by email or SMS

- **Today:** nothing ready-made. The parts exist: `crypto.token()`, a table, and the smtp, mail_api (Resend, Postmark, SendGrid, Mailgun) and sms (Twilio, Vonage) plugins.
- **Works:** a recipe can be written today (read, not run end to end).
- **Security:**
  - A link's token in the URL lands in Quillo's request log (Q-1158).
  - "Used once" needs a compare-and-delete that the program writes.
  - A 6-digit code needs a limit (Q-1151).
  - Mail scanners open links (Outlook's Safe Links), so a link redeemed on GET signs the scanner in, or uses the link up.
- **Recommendation:** built-in. `auth.link(key)` gives `{ url, code }`, and `auth.redeem(...)` takes either, once, stored as an HMAC, with a limit. The link opens a page whose button posts, so a scanner's GET does nothing. Sending stays the program's, with the smtp, mail_api or sms plugins.

### A.7 API keys and bearer tokens

- **Today:** for routes, nothing: a program writes a table, `crypto.token()` and `crypto.sha256` itself. For the MCP door, `mcp_token()` does it all:
  - only an HMAC is kept, under the session key;
  - rows in `_quillo_mcp_tokens` are sealed, so a row changed in the database is no token;
  - tokens expire, and can be listed and revoked;
  - `end_session()` ends them;
  - in a tool, `session()` is the session the token was made in.
- **Works:** the MCP tokens were not probed again here (see FEEDBACK56.md).
- **Recommendation:** built-in, by widening `mcp_token` (decision 5): `auth.token({ user, name, scopes, lasts })`, and a middleware `auth.bearer` that makes `session()` the token's for the request.

### A.8 JWT, JWKS, PASETO and Biscuit

- **Today:**
  - **Core:** `verify_jwt` (src/svcauth.rs:788), used only by `accept_calls { jwt }`. It takes HS256 with a secret, and RS256, ES256 and EdDSA from a JWKS, picked by key type and `kid`. `exp` and `aud` are required, `iss` and `nbf` checked, with 60 s of leeway. The JWKS is kept 10 minutes and fetched by one request at a time.
  - **Plugins:** jwt 2.0.0 (jsonwebtoken 9), jwks 1.0.0, paseto 1.0.0 (pasetors, v4) and biscuit 1.0.0.
- **Works:** core took 1 good token and refused 11 bad ones (Part B). The plugins pass their catalog checks.
- **Security:**
  - jwt takes a token with no `aud` or `iss` (Q-1150).
  - jwks takes one with no `exp`, `aud` or `iss` (Q-1154).
  - paseto has no audience or issuer check, and takes tokens that never expire (Q-1164).
  - biscuit is sound: the crate checks the signatures, and `biscuit_inspect` says it checks nothing.
- **Recommendation:** built-in `auth.jwt(token, rules)`, which opens core's checker to routes. It is code already audited by this report's probe. Signing JWTs, PASETO and Biscuit stay plugins, with Q-1150, Q-1154 and Q-1164 fixed there.

### A.9 HTTP Basic and Digest

- **Today:** nothing. Core cannot decode base64 (`base64_decode` is an unknown name). The guide sends people to the encoding plugin.
- **A recipe without a plugin:** compare the whole header with a precomputed one, and ask for credentials when it does not match:

  ```quillo
  route GET /admin {
      if !crypto.equal(request.headers.authorization ?? "", env("ADMIN_BASIC")) {   // "Basic " + base64("admin:...")
          reply 401 respond(401, "Sign in", { "www-authenticate": "Basic realm=\"admin\"" })
      }
      "the admin page"
  }
  ```

  Run on 0.8.8: no header gives 401 with `www-authenticate: Basic realm="admin"`, `curl -u admin:s3cret` gives the page, and a wrong password gives 401.

  For a table of users, read the header with the encoding plugin's base64 decoder, then check the password with `crypto.verify_password`.
- **Digest:** built on MD5, and no current client needs it. Not recommended. Old devices are served through a proxy.
- **Recommendation:** a recipe in the guide. Basic is for scripts, tools and a gate in front of a staging site.

### A.10 SAML 2.0, as a service provider

- **Today:** plugin saml 1.0.0, 1,046 lines. It has its own XML parser, exclusive canonicalization, RSA and ECDSA signatures with SHA-256 and up (SHA-1 only when allowed), IdP metadata read, SP metadata written, and AuthnRequests by the redirect binding, signed when a key is given.
- **Works:** its 12 catalog checks pass. They include 10 signature-wrapping variants, a comment inside the NameID, a DOCTYPE, an unsigned response, an edited one and another key.
- **Security:**
  - One parser does the checking and the reading. ruby-saml's CVE-2024-45409 (wrapping) and CVE-2025-25291/25292 (two parsers reading one document two ways) are the classes this avoids.
  - A Signature must sit in the element it covers, the ID must be unique, and there must be exactly one assertion anywhere in the document.
  - Audience, recipient, the time window (120 s of skew) and InResponseTo are checked.
  - Replays are remembered per process (Q-1156), and the ACS post has no session (Q-1155).
  - Encrypted assertions are refused, and there is no single logout.
- **Recommendation:** a plugin, audited, with Q-1155 and Q-1156 fixed. Its replay record goes in the database (`auth.once`, Part C).

### A.11 LDAP and Active Directory

- **Today:** plugin ldap 1.0.0 (ldap3 0.12.1, rustls).
- **Works:** yes, with the stand-in directory.
- **Security:**
  - Filters are escaped, and a DN escaper is offered.
  - An empty password is refused before any bind, which closes RFC 4513's unauthenticated bind.
  - A user is found by search and then bound on a connection of its own, and a name that matches two entries is refused.
  - A password needs TLS, except on loopback.
  - The sample tells visitors which accounts exist, and takes the password in the query (Q-1162).
  - One shared connection carries every sign-in, so a slow directory stalls all of them (read).
- **Recommendation:** a plugin.

### A.12 RADIUS

- **Today:** plugin radius 1.0.0: PAP and CHAP, accounting, Status-Server, vendor attributes, and the parts of a server for `udp.listen`.
- **Works:** yes, with the stand-in.
- **Security:**
  - Each request carries a Message-Authenticator, first.
  - The answer's authenticator and source address are checked.
  - A Message-Authenticator in the answer is required only when asked for (Q-1166).
  - There is no RadSec (RADIUS over TLS) for use across the internet.
  - The sample's server drops requests without a Message-Authenticator.
- **Recommendation:** a plugin.

### A.13 Kerberos and SPNEGO

- **Today:** nothing.
- A plugin would need the system's GSSAPI (MIT Kerberos or Heimdal), a keytab, and a KDC to test against, none of which this machine has.
- **Recommendation:** a recipe. A proxy that does SPNEGO (Apache's mod_auth_gssapi, or IIS in front) sets a header with the user, which the program believes only from an address `--trust-proxy` names. That is the shape `client_cert` already has for services (decision 7).

### A.14 mTLS and client certificates

- **Today:** Quillo ends no TLS itself.
  - **Services:** `accept_calls { client_cert: { header } }` reads Envoy's `x-forwarded-client-cert`, or an RFC 2253 subject, from a proxy that `--trust-proxy` names. It was hardened in 0.8.7.
  - **Routes:** nothing. The header is raw text in `request.headers`, which anyone can send unless the program checks the peer itself.
- **Recommendation:** built-in `request.client_cert`, read by the same parser, and only from a trusted proxy.

### A.15 An OAuth 2.0 authorization server

- **Today:** nothing. The MCP door takes a signed-in session or a pasted `mcp_token`. An AI client that expects to find OAuth gets neither.
- **Recommendation:** later, built-in and narrow (decision 6): the authorization code with PKCE, refresh tokens and the device flow, for clients the program registers, with tokens stored as `mcp_token`s are.

### A.16 Calls between services

- **Today:** core, `service ... { auth }` and `accept_calls`: per-caller keys, JWT callers, client certificates and SPIFFE ids, with signed answers.
- **Works and is sound:** the JWT path was probed here (Part B). The other paths have their own tests.
- **Recommendation:** keep it as it is, and reuse its JWT checker and certificate parser for routes (A.8, A.14).

### A.17 Limits on guessing

- **Today:** none for routes. The guide says "none for routes (a `rate_limit` builtin was removed)" (USERGUIDE.md:5088).
- The rate_limit plugin counts in process memory. The instances of one process share it, a restart forgets it, and two servers count apart.
- **Recommendation:** built-in, kept in the database, and on by default in every `auth` check (Q-1151, decision 4).

## Part B: the security review of the auth plugins

Each plugin's source was read whole, and each claim below was probed where a probe could show it. "—" means the point does not apply to that plugin.

| Plugin | Signatures and algorithms | Claims and single use | Timing | Secrets in logs and errors | Limits | Issues |
|---|---|---|---|---|---|---|
| oidc 1.1.0 | RS256 and ES256 only. The JWK's `kty`, `use` and `alg` are checked. RSA keys under 2048 bits refused | state, nonce, PKCE S256, `iss`, `aud` with `azp`, `exp`, `iat`, `nbf`, `at_hash`, `max_age`, RFC 9207's `iss`. The code is single use at the provider | state compared with `!=` (harmless) | Errors carry the provider's error text, not secrets | None. The JWKS is fetched again at most every 30 s for an unknown `kid` | Q-1160, Q-1161 |
| oauth_pkce 1.0.0 | — (no ID token) | Makes state and the verifier. Checking them is the program's (the sample uses `crypto.equal`) | — | The sample keeps the access token in the cookie | — | Q-1167 |
| jwt 2.0.0 | A secret checks HS256 only. The JWKS path refuses HS*, and the crate refuses a key of the wrong family | `exp` required by default. `aud` and `iss` checked only when the token has them | Crate's | — | An unknown `kid` fetches again at most every 30 s | Q-1150 |
| jwks 1.0.0 | The algorithm must fit the key's type. HS* refused | `exp` not required. `aud` and `iss` only when present | — | — | — | Q-1154 |
| paseto 1.0.0 | v4 only, so there is no algorithm to confuse | `exp` checked when present, and tokens that never expire taken. No `aud` or `iss` check | — | — | — | Q-1164 |
| biscuit 1.0.0 | Ed25519 root key. The crate checks every block | Datalog checks, with the time added by the authorizer | — | `biscuit_inspect` checks nothing, and says so | — | None |
| passkeys 1.0.0 | ES256 (DER), EdDSA (strict), RS256 of 2048 bits or more. The COSE key is checked whole | challenge, origin list, rpIdHash, type, user presence. User verification only when asked. Counter, BE and BS | — | — | — | Q-1159 (the runtime's), Q-1163 |
| saml 1.0.0 | Only the configured certificates, never the message's `KeyInfo`. RSA-SHA256, 384 and 512, ECDSA. SHA-1 only when allowed. Exclusive C14N only | One assertion. The Signature in the element it covers. A unique ID. Audience, recipient, NotBefore and NotOnOrAfter with 120 s of skew, InResponseTo. Replays remembered in memory | — | — | — | Q-1155, Q-1156 |
| ldap 1.0.0 | TLS by rustls, with webpki roots or a given CA. `insecure` only when asked | Filters escaped, empty password refused, a name matching two entries refused | Unknown name 0.5 ms, known 3.5 ms | Errors do not carry the password | None. Active Directory's lockout can be set off through the app | Q-1162 |
| radius 1.0.0 | The answer's authenticator is checked. Message-Authenticator by HMAC-MD5 | A Message-Authenticator in the answer only when asked | PAP bytes compared with `==`, over UDP (harmless) | — | — | Q-1166 |
| totp 1.0.0 | — | One step either side. Nothing records a used code | Constant time (the crate's) | — | None | Q-1151, Q-1152 |
| mfa_codes 1.0.0 | — | Single use is the program's delete | Constant time, over every hash | — | — | Q-1153 |
| yubico_otp 1.0.0 | The answer's HMAC-SHA1 is checked when a key is set. The nonce and the OTP must come back | Self-hosted keys: the counter is checked | The signature compared with `!=`, on the client side (harmless) | — | — | None. Tying the key's public id to the user is the program's job |
| argon2, bcrypt, scrypt, legacy_passwords | — | — | Constant-time compares | — | Costs capped: 5 million rounds, scrypt at 2 GiB | Q-1165 |
| password_strength, pwned, passphrase | — | — | — | pwned sends 5 hex characters of a SHA-1, never the password | — | None |
| rate_limit 1.0.0 | — | — | — | — | GCRA in process memory, 200,000 keys at most | Q-1151 |
| cedar, rego | Authorization, not authentication: not reviewed beyond their summaries | | | | | |

Core's `accept_calls { jwt }`, for comparison, with the issuer `https://id.example.com/`, the audience `billing`, and the JWKS on loopback. Each token was sent to `POST /_quillo/service/refund` with `x-quillo-to` and `x-quillo-call`:

| Token | Answer |
|---|---|
| Signed, right `aud`, `iss` and `exp` | 200 `{"ok":{"refunded":100,"by":"shop","via":"jwt"}}` |
| No `aud` | 401 `the bearer token: the token is not for billing (aud)` |
| Another `aud` | 401, the same |
| No `exp` | 401 `the token has no exp: it would be good for ever` |
| Another `iss` | 401 `the token is from another issuer than https://id.example.com/` |
| `exp` in 1970 | 401 `the token has expired` |
| `kid` of `../../../etc/passwd` | 401 ``no key `../../../etc/passwd` in the identity provider's keys`` |
| `alg: none` | 401 `an unsigned token` |
| `alg: None` | 401 `a token signed with None, which is not taken (HS256, RS256, ES256, EdDSA)` |
| HS256 keyed with the JWK's `n` | 401 `an HS256 token, and no secret to check it with` |
| HS256 keyed with the JWK as JSON | 401, the same |
| HS256 keyed with nothing | 401, the same |

So core's checker is the one to open to routes, rather than the plugins'.

Notes on the plugins:

- **SAML.** The plugin's own tests cover the classic attacks: the evil assertion first, after, in Extensions or in a ds:Object; a signed element wrapped; two elements with one ID; the Signature moved; the response wrapped. Its weak points are outside the XML: the replay record (Q-1156) and the session at the ACS (Q-1155). Encrypted assertions, which ADFS and some Entra setups send by default, are refused with a clear error. Single logout is missing.
- **OIDC.** Beyond Q-1160 and Q-1161, a program must decide how to link accounts. The plugin gives `email` and `email_verified` as the provider says. Linking on an unverified email lets anyone who controls that address at a lax provider take over an account. The built-in should link by `(provider, sub)` and use the email only when it is verified.
- **Passkeys.** Discoverable credentials (no user name typed) are supported: an empty `credentials` list. Looking up the credential by its id is the program's job. The plugin cannot know who the user is.
- **LDAP.** An unauthenticated bind (a name with no password, RFC 4513 5.1.2) is refused before it is sent, and the stand-in refuses one too. A whitespace password is sent as given, and refused by the server.
- **TOTP.** A secret shorter than 128 bits is refused by totp-rs, and `totp_secret()` makes 160 bits.
- **Rate limits.** The plugin is correct for one process: the 4 instances of one process share one library and its counts, as the SAML replay test showed for the replay record.

## Part C: a built-in `auth` layer

### C.1 The rules it keeps

- **`session().user` stays** (decision of 2026-11-07). There is no `user()` builtin, keyword, type or table. Each sign-in path ends in `set_session({ user: id })`, or Quillo sets exactly that. Each feature takes a `user:` option and otherwise reads `session().user`. A bearer key makes `session()` the key's for its request, as an MCP token already does in a tool.
- **The database layer removes decisions.** What the layer keeps lives in Quillo's own tables, made as `_quillo_push` and `_quillo_mcp_tokens` are: in the program's database, or in `quillo-system.db` when its databases are one per tenant (`db::system_on_for`). Each table is made on first use, with the right types and indexes for SQLite, PostgreSQL and MySQL. A program never picks a database or a schema for them.
- **No JavaScript names on `window`.** The browser half of passkeys is `B.passkey` in the runtime's private registry, beside `B.notification`. Its code is fetched on the first `passkey.*` call, so the runtime file every page loads does not grow.
- **Nothing slows a program that does not use it.** A program that never names `auth` or `passkey` gets none of this: no endpoint, no table, no runtime code, and no check on its requests. That is how `notification` is built (src/push.rs). Fortunes and the site are benchmarked old against new anyway.
- **What already works is reused.** Core's JWT checker, the MCP tokens' storage, push's tables and sealing, and the plugins' protocol code (C.6).

### C.2 What a program writes

**A password sign-in, with old hashes made again and guessing limited:**

```quillo
route POST /login { form: { email: str, password: str } } with db {
    let u = db.one("select id, password_hash from users where email = ?", lower(normalize(trim(email))))
    let r = auth.password(password, u.password_hash, { limit: email })   // { ok, rehash, wait }; Argon2 or bcrypt
    if r.wait != null { reply 429 `Too many tries. Try again in ${r.wait} s.` }
    if !r.ok { reply 401 "Wrong email or password" }
    if r.rehash { db.exec("update users set password_hash = ? where id = ?", crypto.hash_password(password), u.id) }
    if auth.totp.on(u.id) {
        set_session({ pending: u.id }, { max_age: "5m" })
        reply 303 redirect("/login/code")
    }
    set_session({ user: u.id })
    redirect("/")
}
```

- `limit: email` counts failures for that key, and for the request's address, in `_quillo_auth_limits`: 5 every 15 minutes by default. A success clears the key's count.
- An unknown email (`u` null, so its hash is null) takes as long as a known one, as `crypto.verify_password` does today.

**Two-factor: turning it on, then signing in with a code or a recovery code:**

```quillo
use "qr.qwer"                                                     // the catalog's QR codes, for the picture

route GET /settings/2fa with db {
    let me = session().user ?? reply 401 "Log in first"
    let t = auth.totp.start(me, { issuer: "Corner Shop", account: email_of(me) })    // { secret, uri }: off until confirmed
    <html><body>
        {raw(qr_svg(t.uri, 200)?)}
        <p>Or type {t.secret}</p>
        <form method="post" action="/settings/2fa">
            <input name="code" autocomplete="one-time-code"/><button>Turn on</button>
        </form>
    </body></html>
}

route POST /settings/2fa { form: { code: str } } {
    let codes = auth.totp.confirm(session().user, code)?          // on now, and 10 recovery codes, shown this once
    <html><body><p>Keep these with your passwords:</p><ul>{for c in codes => <li>{c}</li>}</ul></body></html>
}

route POST /login/code { form: { code: str } } {
    let id = session().pending ?? reply 401 "Start again"
    if !auth.totp.check(id, code) && !auth.recovery.use(id, code) { reply 401 "Wrong code" }
    set_session({ user: id })
    redirect("/")
}
```

- `auth.totp.check` refuses a step at or before the last one taken (Q-1152), and is limited per user.
- `auth.recovery.use` deletes the code it matched, in the same statement, so two requests at once cannot both use it.
- The secret is encrypted, and the recovery codes are kept as HMACs (C.5).

**Passkeys, from buttons:**

```quillo
component AddPasskey() {
    state note = ""
    <div>
        <button onclick={() => { note = passkey.add({ name: "This device" }) == "ok" ? "Added" : "Not added" }}>Add a passkey</button>
        <p>{note}</p>
    </div>
}

component PasskeySignIn() {
    state note = ""
    <div>
        <button onclick={() => { let r = passkey.sign_in(); if r == "ok" { navigate("/") } else { note = r } }}>Sign in with a passkey</button>
        <p>{note}</p>
    </div>
}
```

- `passkey.add` binds the new passkey to `session().user`. For a sign-up with no session yet, it binds to `{ user: auth.passkey_user(id) }`, a value sealed on the server, as `notification.user(x)` is.
- `passkey.sign_in` asks for any passkey of this site (discoverable credentials). The server checks it and answers with the session set to `{ user }`.
- The rpId and the origins come from the request's host (a `--trust-proxy` proxy's, behind one). User verification is required. Challenges are used once, from `_quillo_auth_once`.

**"Sign in with Google", or with any provider:**

```quillo
let GOOGLE = auth.provider("google", { client_id: env("GOOGLE_ID"), client_secret: env("GOOGLE_SECRET") })
let WORK = auth.provider("microsoft", { client_id: env("MS_ID"), client_secret: env("MS_SECRET"), tenants: "any" })
let APPLE = auth.provider("apple", { client_id: env("APPLE_ID"), team: env("APPLE_TEAM"), key_id: env("APPLE_KEY_ID"), key: env("APPLE_KEY") })
let STAFF = auth.provider({ issuer: "https://sso.example.com", client_id: env("SSO_ID"), client_secret: env("SSO_SECRET") })

route GET /login/google = auth.start(GOOGLE, { then: "/welcome" })   // off to Google: state, nonce and PKCE kept on the server

route GET /welcome with db {
    let who = auth.finish()?        // once: { via, sub, email, email_verified, name, picture, claims }
    let id = db.one("select user_id from logins where via = ? and sub = ?", who.via, who.sub).user_id ?? sign_up(who)
    set_session({ user: id })
    redirect("/")
}
```

- The provider is given one redirect URI for every provider: `https://site/_quillo/auth/return`. That endpoint takes the provider's GET or POST (Apple's `form_post` too), finds the flow by its `state` in `_quillo_auth_flows`, and checks it against a 10-minute `__Host-quillo_flow` cookie with `SameSite=None`, which a cross-site POST carries (Q-1155). It then exchanges the code, checks the ID token as oidc.qwer does, and sends the visitor to `then`, where `auth.finish()` gives the result once.
- The provider's tokens are never put in the session (Q-1167).
- **Microsoft:** `tenants: "any"` takes `{tenantid}` from the issuer template and checks `iss` against it with the token's `tid` (Q-1160). A list of tenant ids narrows it.
- **Apple:** the ES256 client secret is made from the .p8 key and kept until a day before it ends (Q-1161). The name Apple sends only the first time is put in `who.name`.
- **GitHub:** a preset that reads `/user` and `/user/emails`. `email` is the primary address only if it is verified, else null.
- **Linking:** by `(via, sub)`, never by email alone.

**Sign-in by email link or code:**

```quillo
use "smtp.qwer"

route POST /login/email { form: { email: str } } {
    let e = lower(normalize(trim(email)))
    let l = auth.link(e, { then: "/welcome", lasts: "15m" })          // { url, code }: a link and a 6-digit code, each good once
    send_email(e, "Your sign-in link", `Sign in: ${l.url}\nOr type the code ${l.code}`, "")?
    "Check your email"                                               // the same answer whether there is an account or not
}

route POST /login/email/code { form: { email: str, code: str } } {
    if !auth.code(lower(normalize(trim(email))), code) { reply 401 "Wrong or old code" }
    reply 303 redirect("/welcome")
}
```

- The link opens `/_quillo/auth/link/<token>`: a small page whose button posts, so a mail scanner's GET uses nothing up. It ends at `then`, where `auth.finish()` gives `{ via: "link", email }`.
- `auth.code` lets `auth.finish()` give the same. Codes are limited per address (Q-1151).
- Tokens and codes are kept as HMACs, and the token is hidden in the request log (Q-1158).

**API keys, and routes that take them:**

```quillo
route POST /settings/keys { form: { name: str } } {
    let key = auth.token({ user: session().user, name, scopes: ["orders:read"], lasts: "90d" })   // shown once
    <p>Your key: <code>{key}</code>. It will not be shown again.</p>
}

routes /api | auth.bearer {                                  // Authorization: Bearer <key>
    route GET /orders with db = db.query("select * from orders where user_id = ?", session().user)
}
```

- `auth.bearer` refuses a missing, unknown, expired or revoked key with 401 and `WWW-Authenticate: Bearer`. It then runs the route with `session()` as `{ user }`, and with `request.token` (`{ id, name, scopes }`).
- `auth.tokens(user)` lists a user's keys, and `auth.revoke(id)` ends one. These are `mcp_token`'s storage and rules, widened (decision 5).

**A token from an identity provider, checked with core's rules:**

```quillo
fn partner(r) {
    let claims = auth.jwt(r, { issuer: env("IDP"), audience: "orders", jwks: env("IDP") + "/.well-known/jwks.json" })
    is_err(claims) ? reply 401 { error: claims.error } : merge(r, { claims })
}

routes /partner | partner {
    route GET /stock = stock_for(request.claims.sub)
}
```

**Ending every copy of a session:**

```quillo
route POST /settings/password { form: { old: str, new: str } } with db {
    let me = session().user ?? reply 401 "Log in first"
    let u = db.one("select password_hash from users where id = ?", me)
    if !auth.password(old, u.password_hash, { limit: me }).ok { reply 401 "Wrong password" }
    db.exec("update users set password_hash = ? where id = ?", crypto.hash_password(new), me)
    end_session({ everywhere: true })                      // every device, every copy of the cookie
    set_session({ user: me })                              // and signed in again here
    "Changed"
}
```

**A limit for the program's own routes:**

```quillo
route POST /contact { form: { text: str } } {
    if !auth.limit(`contact:${request.ip}`, { tries: 5, per: "1h" }).ok { reply 429 "Later, please" }
    "Sent"
}
```

The whole surface:

| Name | Gives |
|---|---|
| `auth.password(pw, stored, { limit })` | `{ ok, rehash, wait }` |
| `auth.limit(key, { tries, per })` | `{ ok, wait }` |
| `auth.once(key, until)` | true the first time, false after: for SAML assertion ids, or any one-time value of the program's |
| `auth.totp.start`, `.confirm`, `.check`, `.on`, `.off` | Enrolment, the 10 recovery codes, the check, whether it is on |
| `auth.recovery.use(user, code)`, `auth.recovery.renew(user)` | Single use, and a new set |
| `auth.factors(user)`, `auth.forget(user, id)` | A user's TOTP, passkeys and recovery codes, for a settings page |
| `auth.provider(name or { issuer }, options)`, `auth.start(p, { then })`, `auth.finish()` | OIDC and OAuth sign-in |
| `auth.link(key, { then, lasts })`, `auth.code(key, code)` | Email links and codes (the sending is the program's) |
| `auth.token({ user, name, scopes, lasts })`, `auth.tokens(user)`, `auth.revoke(id)`, `auth.bearer` | API keys, and the middleware that takes them |
| `auth.jwt(request or token, { issuer, audience, jwks or secret })` | The claims, or an error value |
| `auth.passkey_user(id)` | A sealed user for `passkey.add` before there is a session |
| `end_session({ everywhere: true })` | Every copy of the user's session ends |
| `request.client_cert` | The trusted proxy's client certificate identity (A.14) |
| In the browser: `passkey.add({ name, user })`, `passkey.sign_in({ autofill })`, `passkey.state()` | `"ok"`, `"unsupported"`, or an error value |

`auth` and `passkey` are builtin modules, as `crypto` and `notification` are, not new syntax. So no editor grammar changes: the editors need only completions and hover text.

### C.3 The tables

| Table | Holds | Written | Read |
|---|---|---|---|
| `_quillo_auth_factors` | user, kind (`totp`, `passkey`, `recovery`), id, name, data (a TOTP secret, encrypted; a passkey's public key, counter and flags; a recovery code's HMAC), created, last used, last step | Enrolment and each use | Each check |
| `_quillo_auth_once` | key (purpose and value, as an HMAC), until | The first use: an insert, where a duplicate key is the replay | — |
| `_quillo_auth_limits` | key, the GCRA's next time, until | Each attempt, one upsert | The same statement |
| `_quillo_auth_flows` | state (HMAC), provider, nonce, verifier, `then`, the flow cookie's id, until, then the result | `auth.start`, `auth.link` | The return, then deleted by `auth.finish()` |
| `_quillo_mcp_tokens`, widened | As today, plus name and scopes | `auth.token`, `mcp_token` | Each bearer request, with a short cache |
| `_quillo_session_ends` | user, before (a time) | `end_session({ everywhere: true })` | Each request with a session, but only once this table exists. A process keeps its rows in memory and reads them again every 10 s (one indexed read) |

Rows go when they expire, as `_quillo_mcp_tokens`' do. A user's rows go with `auth.forget_user(id)`, for erasure requests.

### C.4 The browser half

- `B.passkey.add` and `B.passkey.sign_in` in src/client/runtime.js, fetched on the first call. Each one:
  1. gets its options from `/_quillo/auth/passkey`;
  2. runs `PublicKeyCredential.parseCreationOptionsFromJSON` and `navigator.credentials.create` (or the `Request` and `get` pair);
  3. posts `credential.toJSON()` back.

  That is the round trip the probe needed `JSON.stringify` for (Q-1159).
- `passkey.sign_in({ autofill: true })` uses conditional mediation: passkeys are offered in the browser's autofill of a field with `autocomplete="username webauthn"`.
- `passkey.state()` says `"unsupported"` where `PublicKeyCredential` is missing, which includes the phone apps' web views until they are wired to the platforms' APIs.
- No names on `window`. The endpoints refuse other sites (`Sec-Fetch-Site`, `Origin`), as `/_quillo/push` does.

### C.5 Keys and secrets

- **Three keys, from `QUILLO_SECRET` by HKDF with fixed labels,** as push's VAPID key is (src/push.rs, `VAPID_LABEL`):
  - one to encrypt TOTP secrets (AES-256-GCM, from `ring`);
  - one for the HMACs of recovery codes, keys, link tokens, codes and once-keys;
  - one to seal rows, so a row changed in the database is refused, as MCP token rows are.
- **Without `QUILLO_SECRET`** a program that uses `auth` warns at start: a restart would end every two-factor enrolment and every key, without a word to anyone.
- **Changing the secret** needs `QUILLO_SECRET_PREVIOUS`, which does not exist today. The service secret has one; the session key has none. With it, rows made under the old key are read and sealed again when used (decision 2).

### C.6 What moves in from the plugins

| From | Lines | Into core as | Changed |
|---|---|---|---|
| passkeys.qwer | 547 | Its CBOR reader, and its authData, COSE-key, flag and clientData checks, as they are | Signatures by `ring` (ECDSA P-256 ASN.1, Ed25519, RSA PKCS#1 2048 to 8192), already linked, in place of p256 0.13, ed25519-dalek and rsa. Challenges from `_quillo_auth_once`. User verification required (Q-1163) |
| oidc.qwer | 466 | Discovery, start, callback, exchange, verification and userinfo, as they are | `ureq` and `ring`, both linked. Issuer templates (Q-1160). Apple's client secret (Q-1161). Flows in a table (Q-1155) |
| totp.qwer | 36 | Written again in about 40 lines on `ring`'s HMAC-SHA1, in place of totp-rs | The step recorded (Q-1152) |
| mfa_codes.qwer | 53 | The alphabet and the `xxxxx-xxxxx` form, as they are | An HMAC under a derived key (Q-1153) |
| argon2.qwer | 103 | `argon2_needs_rehash` | Compared with core's own cost |
| bcrypt.qwer | 20 | Verification only | Adds the `bcrypt` crate (pure Rust). Making bcrypt hashes stays in the plugin |
| oauth_pkce.qwer | 102 | The table of providers | A GitHub preset |
| src/svcauth.rs `verify_jwt` | — | `auth.jwt` | Nothing |
| src/mcp.rs tokens | — | `auth.token`, `auth.bearer` | A name and scopes |
| src/push.rs | — | The table's pattern, and `notification.user`'s sealing, for `auth.passkey_user` | — |

No new crate is needed except `bcrypt`. The plugins stay in the catalog for programs that want them, fixed as the issues say.

### C.7 What stays plugins, and the recipes

- **Plugins:** saml, ldap, radius, yubico_otp, scrypt, legacy_passwords, password_strength, pwned, passphrase, jwt and jwks (for signing), paseto, biscuit, cedar and rego. Each has its own release cycle and its own large crate (ldap3, regorus, zxcvbn's word lists), or a protocol few programs use.
- **Recipes in the guide:**
  - SAML, with `auth.once(r.assertion_id, ...)` for replays (Q-1156), and the request id in a `SameSite=None` cookie of its own (Q-1155).
  - LDAP sign-in, giving one answer for every failure (Q-1162).
  - HTTP Basic (A.9).
  - Kerberos behind a proxy (A.13).
  - Users moved from Django, WordPress or Firebase: verify with the old hash, rehash on success.
  - Password strength and breach checks on sign-up.

### C.8 How other frameworks do it

| Framework | What it gives | What Quillo takes from it |
|---|---|---|
| Rails 8, `bin/rails generate authentication` | Generated code: a User with `has_secure_password` (bcrypt); a `sessions` table, one row per sign-in, with its address and agent; a signed, permanent cookie holding the session's id; password reset by mail; `rate_limit to: 10, within: 3.minutes` on sign-in. No sign-up, no two-factor, no OAuth (that is OmniAuth) | Sessions that can be ended on the server. A limit on sign-in by default |
| Laravel: Fortify, the starter kits (Breeze before), Sanctum, Socialite, Passport | Fortify: a headless backend with two-factor (TOTP and recovery codes), password confirmation, email verification, and throttling through `RateLimiter`. Sanctum: personal access tokens kept as SHA-256, with abilities, and cookies for SPAs. Socialite: OAuth providers. Passport: a whole OAuth 2 server, as a separate package | Keys stored hashed, with scopes. The authorization server as a separate, later piece |
| Django, `contrib.auth` | PBKDF2 by default, a list of hashers, and old hashes upgraded at sign-in. Password validators, a list of common passwords among them. Sessions in the database. A password change ends the other sessions, since a session keeps a hash of the password hash. Two-factor, OAuth, SAML and LDAP are packages (django-otp, allauth, python3-saml, django-auth-ldap) | `rehash` in the check. A password change ends the other sessions |
| Phoenix, `mix phx.gen.auth` | Generated code: `users` and `users_tokens` (tokens for mail kept hashed), a remember-me cookie, magic links as the default sign-in since 1.8 with passwords optional, and "sudo mode" for sensitive pages | Email links as a first-class sign-in. A recent sign-in for sensitive actions (a later `max_age` on the check) |
| Auth.js (NextAuth) | OAuth and OIDC providers by configuration, email links through a database adapter, credentials (discouraged), WebAuthn (experimental), and sessions as encrypted JWTs or in a database | Presets per provider. Email links need a store |
| Lucia | A session library, deprecated in 2025 and made into a guide to writing sessions yourself, with small libraries: Oslo (crypto, OTP, WebAuthn) and Arctic (OAuth clients) | Small, audited parts, rather than a framework that owns the whole flow |

None of them puts SAML, LDAP or Kerberos in its core. The frameworks that generate code (Rails, Phoenix) leave the flow in the program's hands. Quillo's version of that is the few lines in C.2, around parts that Quillo owns.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| FEEDBACK70.md (CSRF by `Sec-Fetch-Site` and `SameSite=Lax`) | The same Lax cookie that keeps other sites from posting as a visitor is why the ACS has no session (Q-1155). `allow_cross_site` lets the post through, but cannot bring the cookie | 1 |
| FEEDBACK56.md (MCP tokens) | `mcp_token()` is already the API-key store a route needs: HMAC only, sealed rows, expiry, listing, revoking. It is just limited to the MCP door (A.7) | 0 (read) |

## New issues

### Q-1150. `jwt_check` and `jwt_verify_jwks` take a token with no `aud` and no `iss` when an audience and an issuer are asked for

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| high | silent | probe (jwt plugin 2.0.0) | 1 |

The guide shows this check for a service's JSON routes (USERGUIDE.md:6653): `jwt_check(token, env("JWT_KEY"), { audience: "orders", issuer: "idp" })`.

- It refuses a token for another audience or issuer, but takes a token that names neither.
- `jwt_sign` adds no `aud`. So every token the program signs with the same key for another purpose (an unsubscribe link, an email check, a token for another service) passes as an `orders` token.
- The cause: jsonwebtoken 9 checks `aud` and `iss` only when the token has them, unless they are in `required_spec_claims`, and the plugin requires only `exp` (jwt.qwer:23).
- `jwt_verify_jwks` uses the same code: a provider's token with no `aud` was taken too.
- Core's `accept_calls { jwt }` refused the same provider token (`the token is not for billing (aud)`).

```quillo
use "jwt.qwer"
let SECRET = "a-test-secret-of-32-bytes-or-more!!"
let bare = jwt_sign({ sub: "mallory" }, SECRET, 3600)?
print(jwt_check(bare, SECRET, { audience: "orders", issuer: "idp" }))
// { sub: "mallory", iat: 1795090424, exp: 1795094024 }: taken
let other = jwt_sign({ sub: "mallory", aud: "billing", iss: "idp" }, SECRET, 3600)?
print(jwt_check(other, SECRET, { audience: "orders", issuer: "idp" }))
// err("InvalidAudience")
```

With a provider's keys (the JWKS on loopback): `jwt_verify_jwks(t, { jwks_url, audience: "billing", issuer: "https://id.example.com/" })` on a token with `iss` and no `aud` gave `{ sub: "shop", iss: "https://id.example.com/", iat: ..., exp: ... }`.

| Impact | |
|---|---|
| **Functionality** | A token made for something else is taken as an API token |
| **User** | Developers who follow the guide get a check that looks complete. Visitors see nothing until someone abuses it |
| **Requests** | No change |
| **Security** | Token confusion. Anyone who holds a token signed with the key (from a link in a mail, or from another service) can call the API as its `sub`. With a provider's JWKS, any token the provider signs without `aud` is taken |

**Suggestion:** in the plugin (2.1.0), add `aud` to `required_spec_claims` when `audience` is given, and `iss` when `issuer` is, and say so in the doc lines. In core, `auth.jwt` uses core's checker, which requires `aud`.

### Q-1151. Nothing limits guessing at a sign-in, a two-factor or a recovery check: one connection made 19,882 two-factor guesses a second and found a valid code within 3 s

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| high | silent | probe (loopback, one kept connection, for 3 s) | 1 |

- **Core has no limit for routes.** The guide says to count in a table of the program's own (USERGUIDE.md:5088).
- **No auth plugin limits itself.**
- **The rate_limit plugin counts in the process.** The instances of one process share its counts, but a restart forgets them, and two servers count apart.
- **The arithmetic.** A TOTP check takes 3 codes of 1,000,000 (one step either side), so at this rate a valid code is found in about 17 s on average (1,000,000 / 3 / 19,882). A 6-digit email or SMS code falls the same way.
- **Passwords.** A password check is slow by design (Argon2id, one hash per core). But nothing stops a list of common passwords tried against every account.
- **LDAP.** Through the LDAP plugin, wrong passwords lock Active Directory accounts.

```quillo
use "totp.qwer"
let SECRET = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP"
route POST /2fa ? { code: str } = totp_verify(SECRET, code)? ? "in" : "wrong"
```

```sh
quillo run probe_guess.qwe --port 31850 &
python3 guess.py     # random 6-digit codes on one connection:
# 59645 guesses in 3 s over one connection: 19882 a second, 1 right
```

| Impact | |
|---|---|
| **Functionality** | Two-factor adds nothing against someone who has the password, unless the program writes its own limit |
| **User** | Developers must design a limit table for each check, and most will not. Visitors' accounts fall to a stolen password alone |
| **Requests** | An attacker's 20,000 requests a second, per connection, all reach the program |
| **Security** | Second factors and short codes are found in seconds. Passwords are sprayed over every account. Directory accounts are locked out from outside |

**Suggestion:** `auth.limit(key, { tries, per })`, kept in `_quillo_auth_limits` (one upsert per attempt, shared by every instance and server). It is applied by default in every `auth` check, per account and per address (decision 4). Until then, a guide section "Limiting guesses", with a table and a compare-and-set.

### Q-1152. A TOTP code is taken again within its 90 seconds: `totp_verify` gives no time step to record

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | silent | probe | 1 |

RFC 6238 §5.2: "the verifier MUST NOT accept the second attempt of the same OTP after the successful validation has been issued for the first OTP". The plugin answers only true or false (totp.qwer:30), so the program cannot record the step it took. With one step either side, a code seen over a shoulder, through a phishing proxy, or in a log is good for up to 90 s, for as many sign-ins as wanted.

```quillo
use "totp.qwer"
let s = totp_secret()
let c = totp_code(s)?
print(totp_verify(s, c), totp_verify(s, c))     // true true
```

| Impact | |
|---|---|
| **Functionality** | One code signs in twice, or more |
| **User** | Nothing that visitors see |
| **Requests** | No change |
| **Security** | A real-time phishing proxy, or someone watching, reuses the code. The second factor is not one-time |

**Suggestion:** `totp_check(secret, code, last_step)` giving `{ ok, step }`, refusing a step at or before `last_step` (1.1.0). The built-in keeps the last step in `_quillo_auth_factors`.

### Q-1153. Recovery codes are hashed with SHA-256 and no salt: one leaked table gives every user's codes within hours

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | silent | reading, and a probe (one hash for every user) | 1 |

- **The hash is the same for everyone.** `mfa_hash(code)` is SHA-256 of `"mfa-recovery:" + code` (mfa_codes.qwer:20).
- **The space is small for a GPU.** A code has 31^10 = 8.2×10^14 values (49.5 bits). One pass over them all, at the 2×10^10 SHA-256 a second published for one current GPU, takes about 11 hours (an estimate, not measured here).
- **One pass finds every user's codes,** since no salt sets them apart.
- **Recovery codes skip the second factor,** so one leaked table takes two-factor away from every account.

```quillo
use "mfa_codes.qwer"
print(mfa_hash("abcde-fghjk"))   // 51132f750dd9837df23f2a1d7a2d0007c07a4d333f7f0ef938d4f17a3ec7534f
print(mfa_hash("ABCDE FGHJK"))   // 51132f750dd9837df23f2a1d7a2d0007c07a4d333f7f0ef938d4f17a3ec7534f
```

| Impact | |
|---|---|
| **Functionality** | None while the table is safe |
| **User** | Nothing that visitors see |
| **Requests** | No change |
| **Security** | A database leak (a backup, an injection elsewhere) gives working recovery codes, which skip two-factor for every account |

**Suggestion:** an HMAC under a server key, as core keeps MCP tokens, or a salt per user with a slow hash (1.1.0, reading old hashes while users move). In the built-in, an HMAC under a key derived from `QUILLO_SECRET`, so the table alone is worth nothing.

### Q-1154. `jwks_verify` takes a token that never expires, and one with no `aud` or `iss` when both are asked for

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | silent | probe (jwks plugin 1.0.0) | 1 |

`jwks_verify(token, jwks, audience, issuer)` clears `required_spec_claims` (jwks.qwer:45), so `exp` is checked only when present. Its doc line says "`exp` and `nbf` with 30 s of leeway", which reads as required. A token the provider signed with no `exp` is good for ever, and `aud` and `iss` are skipped when absent, as in Q-1150.

```quillo
use "jwks.qwer"
let forever = jwks_sign({ sub: "mallory" }, read_file("jwks_sample_key.pem")?, "2026-11-key-1", 0)?
print(jwks_verify(forever, read_file("jwks_sample.json")?, "orders-api", "https://id.example.com/"))
// { sub: "mallory", iat: 1795090438 }: no exp, no aud, no iss, taken
```

| Impact | |
|---|---|
| **Functionality** | Tokens with no end, or for no audience, are taken |
| **User** | Developers read "exp checked" and rely on it |
| **Requests** | No change |
| **Security** | A token that leaks once works for ever. A token the provider made for something else is taken |

**Suggestion:** require `exp`, and `aud` and `iss` when they are given (1.1.0). The guide's JWKS lines point at `auth.jwt`.

### Q-1155. The IdP's POST to the ACS arrives without the session cookie, so the request id kept in `session()` is gone, and only `allow_unsolicited` gets through

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | wrong answer (a sign-in that cannot finish) | browser (headless Chrome) | 1 |

- **The binding.** SAML's HTTP-POST binding sends the visitor back with a form that the IdP's page posts to the ACS. That is a cross-site POST.
- **The cookie.** Quillo's session cookie is `SameSite=Lax`. Browsers send it on a cross-site GET navigation, but not on a cross-site POST.
- **The gate opens, the cookie stays home.** `allow_cross_site("/acs")` lets the post past the CSRF gate (USERGUIDE.md:7219), but `session()` there is `{}`.
- **What the sample does.** The saml plugin's sample keeps the request id "in the session between /login and the ACS post", and passes it as `request_id`. In a browser it is null, and `saml_read_response` refuses: `an unsolicited Response: give request_id, or set allow_unsolicited`.
- **The way out is the weaker setting.** `allow_unsolicited: true` drops the InResponseTo binding. A response an attacker got for their own account can then be posted from the victim's browser (login CSRF), and replays rest on Q-1156's memory alone.
- **Apple too.** Sign in with Apple returns the same way (`response_mode=form_post`) when the name or email is asked for.
- **Why the sample passes.** It fetches responses server to server, never through a browser.

```quillo
allow_cross_site("/acs")
route GET /start { set_session({ req: "request-id-abc123" }); "started" }
route POST /acs { form: { SAMLResponse: str } } { `session at the ACS: ${session().req ?? "none"}` }
route GET /acs-get { `session at a GET callback: ${session().req ?? "none"}` }
```

```html
<!-- served from 127.0.0.1:31842, after the browser visited localhost:31841/start -->
<form id="f" method="post" action="http://localhost:31841/acs"><input name="SAMLResponse" value="PHNhbWxwOlJlc3BvbnNlLz4="></form>
<script>document.getElementById("f").submit()</script>
```

In Chrome: `session at the ACS: none`. The same browser sent to `/acs-get` by a script on 127.0.0.1: `session at a GET callback: request-id-abc123`.

| Impact | |
|---|---|
| **Functionality** | SP-initiated SAML with InResponseTo, and Sign in with Apple with the name or email, cannot finish with Quillo's session |
| **User** | Developers find it only in a real browser with a real IdP, since the sample works. Then they set `allow_unsolicited` |
| **Requests** | No change |
| **Security** | Programs are pushed to the weaker setting: no binding of the response to the browser that asked, so login CSRF |

**Suggestion:**

- **Today, a recipe in the guide:** keep the request id in a cookie of its own, `set_cookie("__Host-saml", req.id, { same_site: "None", max_age: 600 })`, and read it at the ACS with `cookie("__Host-saml")`. `SameSite=None` cookies are sent on a cross-site POST.
- **In core:** the built-in flows keep their state on the server, found by `state`, and tied to a `SameSite=None` flow cookie (C.2).

### Q-1156. SAML's replay record is the process's: after a restart, or on a second server, the same response signs in again

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | silent | probe (the catalog's SAML stand-in) | 2 |

`saml_read_response` keeps the assertion ids it has used in a static map (saml.qwer:846).

- **The instances of one process share it:** 8 posts of one response to `--instances 4` signed in once.
- **A restart empties it:** a response used before the restart signed in again after it.
- **Two servers behind a balancer keep two maps.**
- **The window:** an assertion is good until its NotOnOrAfter (minutes; 5 is common), plus 120 s of skew.

```sh
# run_saml_restart.sh: the test IdP, then probe_saml_replay.qwe; one response, a restart between
first:   signed in as ada@example.com
again:   refused: the assertion _a19aee5e32f956bc7ca9a was already used: a replay
after restart: signed in as ada@example.com
```

The probe's ACS route:

```quillo
use "saml.qwer"
route POST /acs {
    let b = request.json
    let r = saml_read_response(config(), b.response, { request_id: b.id })
    is_err(r) ? `refused: ${r.error}` : `signed in as ${r.name_id}`
}
```

| Impact | |
|---|---|
| **Functionality** | None in normal use |
| **User** | Nothing that visitors see |
| **Requests** | No change |
| **Security** | A captured response (from a proxy's log, a shared machine, a browser extension) signs in again within its window after any deploy, or on another server. With `allow_unsolicited` (Q-1155), nothing else stops it |

**Suggestion:** a guide recipe: keep `r.assertion_id` in a table with a unique key, and refuse when the insert fails. With the built-in: `if !auth.once("saml:" + r.assertion_id, r.not_on_or_after) { reply 401 "Used already" }`. The plugin's doc says its own record is per process.

### Q-1157. A session cookie copied before sign-out keeps working until it expires: there is no way to end a session on the server

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | a gap the guide documents | probe | 1 |

- **The guide says so** (USERGUIDE.md:4822): "a copy of the cookie someone kept stays valid until it expires. To end sessions on the server (log out everywhere, a stolen laptop), keep an id in the session and check it against the database." Each program has to build that, and check it on every request.
- **The cookie lasts 30 days** unless the program sets `max_age`.
- **A password change ends no other session.**
- **A hook already exists.** The session carries a hidden id, `"i"`, that MCP tokens and push subscriptions are bound to. Nothing ends a cookie by it.

```quillo
route POST /login { set_session({ user: 42 }, { max_age: "30d" }); "in" }
route GET /me { let id = session().user ?? reply 401 "Log in first"; `user ${id}` }
route POST /logout { end_session(); "out" }
```

```sh
curl -c jar -X POST localhost:31803/login                   # in
curl -b "__Host-quillo_session=$COPY" -X POST localhost:31803/logout
#   set-cookie: __Host-quillo_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Lax
curl -b "__Host-quillo_session=$COPY" localhost:31803/me    # user 42
```

| Impact | |
|---|---|
| **Functionality** | "Sign out everywhere", "end that device's session" and "a new password signs out the others" need a table of the program's own, checked on each request |
| **User** | A visitor whose laptop or cookie is stolen cannot end the thief's session, even by changing the password |
| **Requests** | None today. The fix adds one cached read per request with a session, only for programs that use it |
| **Security** | A stolen cookie (malware, a shared computer, a log) works for up to 30 days, whatever its owner does |

**Suggestion:** `end_session({ everywhere: true })`, with a stamp per user in `_quillo_session_ends`, read only once the table exists (decision 3).

### Q-1158. Quillo's request log writes the whole query string: a password, a link's token or an OAuth code in a URL goes to the log

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | silent | probe (the ldap sample, with the request log on) | 1 |

- **Seen in the log.** The ldap plugin's sample takes the password in the query (`route POST /sign-in ? { user: str, password: str }`), and the request log printed `POST /sign-in?user=nobody&password=guess -> 200 (0.5ms)`.
- **Other secrets travel in URLs by design:** the token of an email sign-in or password-reset link; `code` and `state` from OAuth; `SAMLRequest` in SAML's redirect binding.
- **Logs live long.** They go to a service manager or a log host, are kept for months, and are read by more people than the database.
- **Others filter.** Rails filters `password`, `token`, `secret` and similar names from its logs by default.

```sh
quillo run ldap.qwe          # with the catalog's LDAP stand-in, request log on
curl -G -X POST --data-urlencode "user=nobody" --data-urlencode "password=guess" localhost:31825/sign-in
# stderr: POST /sign-in?user=nobody&password=guess -> 200 (0.5ms)
```

| Impact | |
|---|---|
| **Functionality** | None |
| **User** | Operators keep passwords and live tokens in their logs without knowing it |
| **Requests** | No change |
| **Security** | Anyone who reads the logs can use a link not yet used, or a password sent in a URL |

**Suggestion:** the request log writes `[hidden]` for the values of query parameters whose names mark a secret (`password`, `pass`, `token`, `code`, `secret`, `key`, `otp`, `state`, `SAMLRequest`, `SAMLResponse`, `ticket`), and for the built-in's link paths. The scan is benchmarked like any change. The ldap sample takes a form, not a query (Q-1162).

### Q-1159. A page's object passed to a `server fn` arrives as `{}`: a passkey's credential loses every field

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | silent | browser (headless Chrome, CDP virtual authenticator) | 2 |

- **What happened.** `js("navigator.credentials.create", ...)` gives the page's `PublicKeyCredential`. Passed to a server fn as it is, it arrived as an empty map, and the plugin answered `the credential's type is null, and should be public-key`.
- **Why.** The runtime packs a map from `Object.keys(v)` (src/client/runtime.js:5939). A credential's fields are getters on its prototype, so it has no keys of its own, and its `toJSON()` is never called. `toJSON()` gives exactly what the plugin expects.
- **The workaround.** `json.parse(js("JSON.stringify", cred))?` calls `toJSON()`. With that, the whole ceremony worked: registered (ES256, user verified) and signed in (counter 2).
- **Who else is hit.** `PushSubscription` and `GeolocationPosition` are built the same way: getters, and a `toJSON()`.

```quillo
import "../plugins/passkeys.qwer" use *
// reg_options() keeps the challenge in the session; reg_finish(cred) calls passkeys_register_verify
component Passkey() {
    state out = "ready"
    <div>
        <button id="reg" onclick={() => {
            let opts = js("PublicKeyCredential.parseCreationOptionsFromJSON", reg_options())
            let cred = js("navigator.credentials.create", { publicKey: opts })
            out = json.stringify(reg_finish(cred))
            // {"error":"the credential's type is null, and should be public-key"}
            // with reg_finish(json.parse(js("JSON.stringify", cred))?):
            // {"registered":"1C3Hhjzc...","algorithm":"ES256","uv":true}
        }}>Register</button>
        <p id="out">{out}</p>
    </div>
}
```

| Impact | |
|---|---|
| **Functionality** | Passkeys, and other browser objects built of getters, reach the server empty |
| **User** | Developers get a puzzling error from the server ("type is null"), and conclude that passkeys need JavaScript of their own |
| **Requests** | One wasted call each time |
| **Security** | None directly. It pushes programs toward hand-written scripts |

**Suggestion:** in `pack`, a page's object that is not a Quillo map and has a `toJSON` method is packed from what `toJSON()` gives. The built-in `passkey.*` makes this moot for passkeys.

### Q-1160. The oidc plugin cannot sign in with Microsoft's multi-tenant endpoints, whose issuer is a template

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | wrong answer (refused) | probe with a local stand-in, and Microsoft's public metadata | 1 |

- **The plugin's rule.** `oidc_discover` requires the metadata's `issuer` to equal the URL asked for (oidc.qwer:87), as OIDC Discovery says.
- **Microsoft's metadata** (read 2026-11-19):
  - `common` and `organizations` say `https://login.microsoftonline.com/{tenantid}/v2.0`;
  - `consumers` says a fixed tenant id.
- **So** "Sign in with Microsoft" for any work or personal account is refused at discovery. Only one tenant's own issuer works.
- **The right check** takes the template, puts the token's `tid` in it, and compares the result with the token's `iss`.

```quillo
use "oidc.qwer"
// ms/common/v2.0/.well-known/openid-configuration served on 31830, its issuer "http://127.0.0.1:31830/{tenantid}/v2.0"
print(oidc_discover("http://127.0.0.1:31830/common/v2.0"))
// err("the provider says its issuer is \"http://127.0.0.1:31830/{tenantid}/v2.0\", and http://127.0.0.1:31830/common/v2.0 was asked for")
```

| Impact | |
|---|---|
| **Functionality** | No Microsoft sign-in for "any work or school account" or "any account": only one company's tenant |
| **User** | Developers try `common`, as Microsoft's own pages show, and get an issuer error |
| **Requests** | No change |
| **Security** | None: refusing is safe. A hand-made way around it (skipping the issuer check) would not be |

**Suggestion:** a Microsoft preset in the plugin and in the built-in. It reads `{tenantid}` from the template, requires `tid` in the token, checks `iss` against the template filled with it, and takes an optional list of the tenants allowed.

### Q-1161. Sign in with Apple cannot be done with the catalog: its client secret is an ES256 JWT that no plugin signs

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| medium | missing | reading, and Apple's public metadata | 0 |

- **Apple's token endpoint takes only `client_secret_post`** (read 2026-11-19). The secret is a JWT signed ES256 with the developer's .p8 key, with `iss` (the team id), `iat`, `exp` (at most six months), `aud` of `https://appleid.apple.com`, and `sub` (the client id).
- **No plugin signs one.** The jwt plugin signs HS256 only, jwks signs RS256 only, and apns's `apns_token` signs push's fixed claims.
- **Two more turns:**
  - Apple returns by `form_post` when the name or email is asked for, where Quillo's session is not sent (Q-1155).
  - It sends the user's name only on the first sign-in, in a `user` form field outside the ID token.

```sh
curl -s https://appleid.apple.com/.well-known/openid-configuration
# issuer https://appleid.apple.com | token_endpoint_auth_methods_supported ['client_secret_post']
# response_modes_supported ['query', 'fragment', 'form_post']
```

| Impact | |
|---|---|
| **Functionality** | No Sign in with Apple. Apple asks iOS apps that offer other social sign-ins to offer it too |
| **User** | Developers would need a Rust plugin of their own |
| **Requests** | No change |
| **Security** | None now. A hand-written signer would be a new risk |

**Suggestion:** `jwt_sign_es256(claims, p8_pem, kid)` in the jwt plugin. The built-in Apple preset makes the secret from `APPLE_KEY`, keeps it until a day before it ends, takes the `form_post` return through its server-side flow, and keeps the first-time name.

### Q-1162. The ldap sample tells a visitor whether an account exists, and takes the password in the query; `ldap_sign_in` answers seven times faster for an unknown name

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| low | wrong answer (accounts can be listed) | probe (the catalog's LDAP stand-in) | 2 |

- **The answers.** `POST /sign-in` sends the plugin's answer to the visitor:
  - `{"ok":false,"reason":"no such user"}` for an unknown name;
  - `"wrong password"`, with the account's DN, for a known one;
  - `"account locked out"` for a locked one.
- **The password** is a query parameter, which the request log prints (Q-1158).
- **The timing.** Even with the reasons hidden, the server took 0.5 ms for an unknown name (a search) and 3.5 ms for a known one (a search and a bind).
- **What was right.** Filter injection was escaped: `ad*` and `ada)(|(sAMAccountName=*` found no one.

```sh
for u in "ada:guess" "nobody:guess" "lou:Correct-Horse-1" "ad*:guess"; do
  curl -s -G -X POST --data-urlencode "user=${u%%:*}" --data-urlencode "password=${u#*:}" localhost:31825/sign-in; echo
done
# {"ok":false,"reason":"wrong password","dn":"cn=Ada Lovelace,ou=People,dc=quillo,dc=test"}
# {"ok":false,"reason":"no such user"}
# {"ok":false,"reason":"account locked out","dn":"cn=Lou Locked,ou=People,dc=quillo,dc=test"}
# {"ok":false,"reason":"no such user"}
```

| Impact | |
|---|---|
| **Functionality** | None |
| **User** | Developers copy a sample that tells attackers which accounts exist, and which are locked |
| **Requests** | No change |
| **Security** | Accounts listed by text and by timing, locked accounts shown, passwords in logs |

**Suggestion:**

- **The sample:** take a form, and answer one text for every failure. The reasons stay in the program's own log.
- **The plugin:** bind a fixed fake DN when the search finds no one, so both cases take a bind's time.

### Q-1163. The passkeys plugin does not require user verification unless asked, and calls an attestation by any self-signed certificate `basic`

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| low | silent | reading | 0 |

- **User verification.** `require_user_verification` is false by default (passkeys.qwer:333), while the options the plugin makes ask for `"preferred"`. A passkey that replaces a password should prove the person (a PIN or a biometric), not only that someone touched the key. With the default, a security key without a PIN signs in as the only factor.
- **Attestation.** A `packed` attestation with `x5c` is checked with the certificate's own key, with no chain, and reported as `attestation: "basic"` with the certificate. The doc says the chain is the caller's to judge, but `basic` reads as "attested".

| Impact | |
|---|---|
| **Functionality** | Sign-in with presence only, by default |
| **User** | Nothing that visitors see |
| **Requests** | No change |
| **Security** | A found or stolen security key without a PIN signs in on its own. A program that trusts `basic` trusts software authenticators |

**Suggestion:** require user verification by default, with an option to turn it off where the passkey is a second factor. Report `attestation: "unverified-certificate"`.

### Q-1164. `paseto_decrypt` and `paseto_verify` take tokens that never expire, and have no audience or issuer check

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| low | silent | probe | 1 |

`rules()` allows tokens that never expire (paseto.qwer:53). So a token made with `expires_in: 0` (the plugin's own "never expires") is taken by every check. The API also has no way to ask for an `aud` or an `iss`: a `v4.public` token for another service, signed with the same key, passes.

```quillo
use "paseto.qwer"
let k = paseto_local_key()?
let t = paseto_encrypt({ sub: "mallory", aud: "billing" }, k, 0)?
print(paseto_decrypt(t, k))
// { iat: "2026-11-19T12:13:44Z", nbf: "2026-11-19T12:13:44Z", sub: "mallory", aud: "billing" }: taken
```

| Impact | |
|---|---|
| **Functionality** | Every checker takes tokens with no end, and for any audience |
| **User** | Developers cannot ask for an audience at all |
| **Requests** | No change |
| **Security** | One key shared by two services lets each one's tokens pass at the other. A leaked token never ends |

**Suggestion:** an options map with `audience`, `issuer` and `allow_non_expiring` (false by default), in 1.1.0.

### Q-1165. `bcrypt_hash` and `bcrypt_verify` read only the first 72 bytes of a password, and say nothing

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| low | silent | probe | 1 |

bcrypt reads 72 bytes, and the bcrypt 0.15 crate cuts the rest without a word. A 90-byte passphrase was hashed with `bcrypt_hash`, then verified with another 90-byte passphrase that shares its first 72 bytes. For checking imported hashes this is bcrypt's own rule, and cannot change. For making new hashes, the plugin should refuse or warn, and point at `crypto.hash_password`.

```quillo
use "bcrypt.qwer"
let a = repeat("correct horse battery staple ", 3)
let h = bcrypt_hash(a + "one", 10)?
print(len(a + "one"), bcrypt_verify(a + "two", h))     // 90 true
```

| Impact | |
|---|---|
| **Functionality** | Two different long passwords are one |
| **User** | People with long passphrases get less than they think |
| **Requests** | No change |
| **Security** | The end of a long password adds nothing. Weak only where the first 72 bytes are guessable |

**Suggestion:** `bcrypt_hash` refuses a password over 72 bytes, or uses a later crate's non-cutting hash. The doc says so for `bcrypt_verify`.

### Q-1166. The RADIUS client does not require a Message-Authenticator in answers unless asked

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| low | silent | reading | 0 |

- **The attack.** BlastRADIUS (CVE-2024-3596) forges an Access-Accept out of an Access-Reject by an MD5 collision, where answers carry no Message-Authenticator.
- **What the plugin does:**
  - It sends one first in every request. Servers that follow RFC 3579 must check it, which blocks the attack's change to the request.
  - It checks the one in an answer that has one.
  - It takes an answer without one, unless `require_message_authenticator: true` (radius.qwer:428).
- **The guidance** since BlastRADIUS, and FreeRADIUS 3.2.5's defaults, is to require it.
- **The sample's server side** already drops requests without one.

| Impact | |
|---|---|
| **Functionality** | None with servers of 2024 or later, which send it |
| **User** | Developers do not know the option exists |
| **Requests** | No change |
| **Security** | Against a server that does not check requests, an attacker on the path can turn a reject into an accept |

**Suggestion:** true by default, with an option to turn it off for an old server.

### Q-1167. The oauth_pkce sample keeps the provider's access token in the session, which anyone holding the cookie can read

| Severity | Kind | Detected by | Attempts |
|---|---|---|---|
| low | silent | reading, and the session cookie decoded | 1 |

- **The sample** ends with `set_session({ github_token: json.parse(answer.body)?.access_token })` (oauth_pkce.qwe:26).
- **A Quillo session is signed, not encrypted.** Its payload is base64url JSON: the probe's cookie decoded to `{"e":1797682511,"d":{"user":42},"a":1,"i":"2b9997ad50a4554aefd6c73140b0c9e0"}`. The guide says to keep secrets out of it (USERGUIDE.md:4809).
- **Where the token goes:** a GitHub token with `read:user user:email` lands in the browser's cookie store, in proxies that log cookies, and in every copy of the cookie (Q-1157).

```sh
# any session cookie: the part before the dot is the session, as JSON
python3 -c "import base64,sys; v=sys.argv[1].split('.')[0]; print(base64.urlsafe_b64decode(v+'='*(-len(v)%4)))" "$COOKIE"
```

| Impact | |
|---|---|
| **Functionality** | None |
| **User** | Developers copy a sample that puts a third party's token where the guide says not to |
| **Requests** | About 40 bytes more on every request, in the cookie |
| **Security** | The visitor's GitHub token is readable by anyone who gets the cookie, and lasts as long as GitHub lets it |

**Suggestion:** the sample keeps the token on the server, in a table found by the user, with only the user id in the session. The built-in never puts a provider's tokens in the session.

## Proposed order of work

After FEEDBACK75 lands, and main is released and in sync:

0. **Plugin releases, which need no Quillo release.** These are in `~/dev/quillo-website/plugins`, for whoever owns the catalog:
   - jwt 2.1.0 (Q-1150, and the ES256 signer of Q-1161), jwks 1.1.0 (Q-1154), paseto 1.1.0 (Q-1164);
   - totp 1.1.0 (Q-1152), mfa_codes 1.1.0 (Q-1153);
   - oidc 1.2.0 (Q-1160), passkeys 1.1.0 (Q-1163), bcrypt 1.1.0 (Q-1165), radius 1.1.0 (Q-1166);
   - the ldap, oauth_pkce and saml samples (Q-1162, Q-1167, and the Q-1155 and Q-1156 recipes).
1. **Core, small and alone:**
   - Q-1159: the runtime packs `toJSON()`.
   - Q-1158: query secrets hidden in the request log.
   - Benchmarked old against new, as every change is.
2. **Phase 1 of `auth`:**
   - `auth.limit` and `auth.once` with their tables (Q-1151, Q-1156);
   - `auth.password` with `rehash`;
   - TOTP and recovery codes (moving totp and mfa_codes in, Q-1152, Q-1153);
   - `end_session({ everywhere: true })` (Q-1157);
   - `QUILLO_SECRET_PREVIOUS` (decision 2).
3. **Phase 2 of `auth`:**
   - providers and flows (moving oidc in, Q-1155, Q-1160, Q-1161);
   - passkeys (moving passkeys in, the browser half);
   - email links and codes;
   - `auth.token` and `auth.bearer`;
   - `auth.jwt`;
   - `request.client_cert`.
4. **The guide and the book, with each phase:** an "Authentication" chapter with the C.2 programs, and the recipes of C.7. The editors' completion lists too.
5. **If decided:** the authorization server (decision 6). Later, perhaps a `quillo new --auth` starter, as Rails 8 and Phoenix generate one.

## Decisions for the user

### 1. Should Quillo have a built-in `auth` layer for the everyday set?

**Yes. Every failure measured in the everyday set needs something only core has: a shared database record, a limit, the session, or the runtime. The protocol code is already written, in plugins that pass their tests.**

| Impact | The `auth` and `passkey` builtins (Part C) |
|---|---|
| **Functionality** | Passwords with `rehash`, TOTP and recovery codes, passkeys, sign-in with Google, Microsoft, Apple, GitHub or any OIDC issuer, email links and codes, API keys, and JWT checks, each in a few lines that end in `session().user` |
| **User** | Developers write no JavaScript and no flow state, and design no tables or limits. Visitors get passkeys and providers that work on every server, and codes that cannot be reused |
| **Requests** | None without `auth`. With it: one database round trip per attempt, one or two calls out per provider sign-in, and two POSTs per passkey ceremony |
| **Security** | Limits on by default, single use across servers, secrets encrypted or kept as HMACs, sessions that can be ended. Fixes come with Quillo releases |

What is lost: core grows by an estimated 2,500 to 3,500 lines with tests (most of it moved plugin code), a provider's change needs a Quillo release, and the program gives up its own tables for these data.

**My recommendation:** build it in the two phases above, as `auth` on the server and `passkey` in the browser.

**The question:** shall I plan the `auth` layer as Part C describes it, in the two phases of the order of work?

### 2. Where its data live, and how its keys are made

**In Quillo's own tables (`_quillo_auth_*`), beside `_quillo_push` and `_quillo_mcp_tokens`, with keys derived from `QUILLO_SECRET`. And a `QUILLO_SECRET_PREVIOUS`, so that changing the secret does not end everyone's two-factor.**

| Impact | Quillo's tables, keys from `QUILLO_SECRET`, and its rotation |
|---|---|
| **Functionality** | The same on SQLite, PostgreSQL and MySQL, with no schema to write. The secret can be changed without ending sessions, keys, two-factor or push subscriptions |
| **User** | Developers set one secret, as they already do. Visitors keep their enrolments across a change of secret |
| **Requests** | No change. Rows sealed under the old key are sealed again when next used |
| **Security** | A leaked database alone gives no TOTP secret, no working recovery code and no key. A leaked `QUILLO_SECRET` with the database gives them all, as it already gives sessions |

What is lost: the program cannot keep these data in its own `users` table, and backups must keep `QUILLO_SECRET` with the database, or two-factor ends at a restore.

**My recommendation:** as described. `QUILLO_SECRET_PREVIOUS` also helps every program that has sessions, auth or not.

**The question:** shall the auth tables derive their keys from `QUILLO_SECRET`, with `QUILLO_SECRET_PREVIOUS` added so that the secret can be changed?

### 3. Ending a user's sessions on the server

**Yes, as `end_session({ everywhere: true })`, also used after a password change. A stamp per user is kept in `_quillo_session_ends`, and read only by programs that have used it once.**

| Impact | `end_session({ everywhere: true })` |
|---|---|
| **Functionality** | "Sign out everywhere", "a new password signs the others out", and an operator ending a stolen session |
| **User** | Visitors can act on a lost device. Developers write one call, not a session table |
| **Requests** | For a program that uses it, one indexed read per process every 10 s, and a check in memory per request. Nothing for the others |
| **Security** | A stolen cookie dies when its owner says so, within 10 s on other servers |

What is lost: stateless sessions, for programs that use it (they then need a database), and up to 10 s during which another server still takes an ended session.

**My recommendation:** build it in phase 1.

**The question:** shall `end_session({ everywhere: true })` end every copy of a user's session, at the cost of a cached read for the programs that use it?

### 4. Limits on guessing

**Yes, in core, kept in the database, and on by default in every `auth` check: 5 failures per 15 minutes per account and per address, and stricter for 6-digit codes. `auth.limit` is offered for the program's own routes.**

| Impact | `auth.limit`, on by default |
|---|---|
| **Functionality** | Every check is limited without code. Programs limit their own routes (contact forms, exports) in one line |
| **User** | Visitors who mistype get a wait, not a lockout. Attackers get 5 tries, not 20,000 a second |
| **Requests** | One upsert per attempt on the routes that check. Nothing elsewhere |
| **Security** | Q-1151's 17-second break of two-factor takes about two years at 5 tries per 15 minutes (333,333 guesses on average). Password spraying slows to the limit |

What is lost: a busy sign-in route makes one more write per attempt, and a shared office address can hit its limit (the per-address limit is set higher than the per-account one, for that reason).

**My recommendation:** build it first, in phase 1. It also brings back what the removed `rate_limit` builtin did, but shared by every server.

**The question:** shall every `auth` check be limited by default, with `auth.limit` for the program's own routes?

### 5. API keys: widen `mcp_token`, or keep it apart

**Widen it. `mcp_token()` already stores keys the right way. `auth.token()` gives the same keys a name and scopes, and `auth.bearer` takes them on routes. `mcp_token` stays as a name for what it does today.**

| Impact | One kind of key for the MCP door and for routes |
|---|---|
| **Functionality** | Keys for scripts, CI and partners on any route, with scopes, expiry, listing and revoking. The same key can open the MCP door |
| **User** | Developers learn one function. Visitors manage one list of keys |
| **Requests** | One read per bearer request, cached for a short time, as MCP tokens are today |
| **Security** | Keys kept as HMACs in sealed rows, ended with the session or one by one. Scopes keep a CI key from doing what a person can |

What is lost: a key made for an AI client could reach routes, unless its scopes say otherwise. Keys made by `mcp_token()` keep reaching only the door.

**My recommendation:** widen it, in phase 2.

**The question:** shall `mcp_token()` become one case of `auth.token()`, with keys for routes too?

### 6. An OAuth 2.0 authorization server

**Later, after phase 2, and narrow: the authorization code with PKCE, refresh tokens and the device flow, for clients the program registers. Tokens are kept as keys are (decision 5). It is what remote AI clients expect from an MCP server, and what "Sign in with my app" needs.**

| Impact | A narrow authorization server in core |
|---|---|
| **Functionality** | Other apps, CLIs and TVs (the device flow) sign in with a Quillo app. AI clients connect to the MCP door without a pasted token |
| **User** | People approve an app on a consent page instead of copying a key |
| **Requests** | The flow's few requests per sign-in. Nothing for programs that do not turn it on |
| **Security** | A new surface: redirect URI checks, consent, token lifetimes. Narrow scope keeps it small. No implicit flow, no password grant |

What is lost: core grows further (an estimated 1,500 lines with tests), and dynamic client registration and federation are left out.

**My recommendation:** plan it after phase 2, if the MCP door's users ask for it. The pasted token works meanwhile.

**The question:** shall a narrow authorization server be planned after phase 2, or left out?

### 7. The enterprise protocols

**SAML, LDAP and RADIUS stay plugins, fixed as Q-1155, Q-1156, Q-1162 and Q-1166 say. Kerberos and client certificates on routes are read from a trusted proxy's header, in core (`request.client_cert`, and a user header named for SPNEGO), with a guide recipe for the proxy.**

| Impact | Plugins for the enterprise set, and trusted proxy headers in core |
|---|---|
| **Functionality** | Company sign-in by SAML, AD, RADIUS or Kerberos, the last through Apache or IIS in front |
| **User** | Developers in companies get recipes that work with what IT already runs |
| **Requests** | None |
| **Security** | The audited plugins stay apart from core's releases. A proxy's header is believed only from `--trust-proxy` addresses, with the parser hardened for services in 0.8.7 |

What is lost: SAML is not built in, so programs install a plugin and build it with cargo once. Kerberos needs a proxy.

**My recommendation:** as described.

**The question:** shall SAML, LDAP and RADIUS stay plugins, with Kerberos and client certificates read in core from a trusted proxy's header?

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| Took core's TOTP as given | The brief said `crypto.totp_secret` exists | ``error: `crypto` has no function `totp_secret`: Quillo removed it in 0.5.0`` | 1 run. The report's correction |
| Posted the ldap sample's sign-in as a form | A sign-in takes a form | `bad request: no route pattern matches this request`: the sample reads a query pattern (`?`), which then put the password in the log (Q-1158) | 1 run |
| Passed `navigator.credentials.create`'s result to a server fn | It is an object, and objects become maps | `the credential's type is null` (Q-1159) | 1 browser run |
| Ran `jwt_verify_jwks` on tokens minted earlier | The same tokens as the core test | `ExpiredSignature`: they were made to last 5 minutes | 1 run |
| Signed a JWKS test token with the sample JWKS's first key | The first key is the usual one | `the token's algorithm RS256 does not go with the key's type`: the first key is EC | 1 run |
| `reply respond(401, ...)` in the Basic recipe | `reply` ends a request, and `respond` makes one | `reply status must be an int, got response`: it is `reply 401 respond(...)` | 1 check |

## Limitations

- **No real identity provider was used.** Google, Microsoft, Apple and GitHub flows were not run end to end. Only their public discovery documents were read, and the conclusions on Microsoft and Apple come from those and from the code.
- **Kerberos** was not tested: there is no KDC here. **mTLS** at the TLS level was not tested, since Quillo ends no TLS. The services' header path has its own tests, not repeated here.
- **SAML** ran only against the catalog's test IdP, not ADFS, Okta or Entra. Encrypted assertions were not tried, since the plugin refuses them by design.
- **LDAP** ran only against the stand-in, not Active Directory. AD's own anonymous-bind settings were not tested.
- **RADIUS:** the BlastRADIUS collision was not attempted. Q-1166 is from reading.
- **Passkeys** ran only in headless Chrome with a virtual authenticator. Safari, Firefox, real platform authenticators, phones as authenticators, and attestation other than `none` in a browser were not tried. The replay of a captured (response, cookie) pair is reasoned, not run.
- **The guessing rate** is for loopback and one connection, on this Mac while other agents built Quillo. It is a floor, not a ceiling.
- **The recovery-code cracking time** is an estimate from published GPU rates, not measured.
- **Line counts and size estimates** for the built-in were not counted.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Core's `accept_calls { jwt }` against 11 bad tokens: `alg: none`, `None`, HS256 keyed with the JWK's `n`, with its JSON and with nothing, no `aud`, another `aud`, no `exp`, another `iss`, expired, and a path in `kid` | All refused with 401, each with its reason |
| `jwt_check` with another `aud`, another `iss`, or no `exp` | Refused: `InvalidAudience`, `InvalidIssuer`, `Missing required claim: exp` |
| `jwks_verify` on a token whose `exp` is in 2001 | `ExpiredSignature` |
| LDAP sign-in with `ad*` and `ada)(|(sAMAccountName=*` | "no such user": escaped |
| LDAP sign-in with an empty password | Refused before any bind (the sample's `empty: false`, in the catalog checks) |
| One SAML response posted 8 times to 4 instances of one process | One sign-in, 7 replays refused: the instances share the plugin's memory |
| SAML wrapping variants, a comment in the NameID, a DOCTYPE, SHA-1, unsigned, edited, another key | All refused (the plugin's `/refused` and `/wrapping`, in the catalog checks) |
| OIDC: expired, other audience, other issuer, wrong nonce, forged, `alg: none`, HS256, wrong `at_hash`, other `sub`, denied, stolen state, wrong verifier, a code used twice | All refused (the plugin's `/refused`, in the catalog checks) |
| Passkeys through `js()` and the JSON round trip | Registered (ES256, user verified) and signed in (counter 2), with no script of the program's own |
| A provider sending the visitor back by a cross-site GET | The session was there, so OIDC's usual query return works with Quillo's session |
| `crypto.verify_password` with a bcrypt hash, null, or an empty password | false each time. Argon2id at m=19456, t=2, p=1 |
| The session cookie | `__Host-quillo_session`, HttpOnly, Secure, SameSite=Lax. `/logout` sent `Max-Age=0` |
| `totp_verify` with codes two steps away | Refused: the window is one step either side |
| HTTP Basic by a precomputed header and `crypto.equal` | 401 with `www-authenticate`, the page with the right password, 401 with a wrong one |

## Scripts

Everything is in `.git/wip/feedback78/`:

- `plugins/`: the 23 plugins and their samples, copied from the catalog, with `.quillo/native/` built by `quillo build all_try.qwe` (behind `.git/wip/coverage-087/cargo.lock`), `test.mjs` and `catalog.json` copied, and `standins/`.
- `plugins/standin_run.mjs`: starts one stand-in on a port and prints its env.
- `plugins/probe_tokens.qwe`: Q-1150, Q-1152, Q-1153, Q-1164.
- `plugins/probe_jwks.qwe`: Q-1154.
- `plugins/probe_totp.qwe` and `totp_codes.py`: the window.
- `plugins/probe_guess.qwe` and `guess.py`: Q-1151.
- `plugins/probe_bcrypt.qwe`: Q-1165.
- `plugins/probe_saml_replay.qwe`, `run_saml_replay.sh` and `run_saml_restart.sh`: Q-1156.
- `plugins/run_ldap.sh`: Q-1158, Q-1162.
- `plugins/probe_ms.qwe` and `ms/`: Q-1160.
- `core/billing.qwe`, `mint.qwe`, `attack_tokens.py` and `run_svc.sh`: core's JWT check.
- `core/probe_jwt_jwks.qwe`: Q-1150's JWKS half.
- `core/session.qwe` and `run_session.sh`: Q-1157.
- `core/basic.qwe` and `basic_recipe.qwe`: HTTP Basic.
- `core/pw_core.qwe` and `totp_core.qwe`: core's passwords, and the missing TOTP.
- `browser/pk.qwe`, `samesite.qwe`, `post.html`, `get.html`, `drive.mjs` and `run_browser.sh`: Q-1155, Q-1159, and passkeys working.

Every server ran in its own process group with a deadline (`perl -e 'setpgrp; alarm N; exec @ARGV'`) on ports 31801 to 31860, and was stopped by its group.
