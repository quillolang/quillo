# Compliance: SOC 2 and Quillo

SOC 2 is an audit of an organisation, not of a programming language. An auditor looks at
the controls of a company that runs a service, and at whether they worked over a period.
This document is for that company, when its service is written in Quillo, and for its
auditor. It says:

- which Trust Services Criteria Quillo's features help meet, and how to turn them on;
- what stays the operator's to do;
- how Quillo itself is developed and checked;
- the security risks that are known and accepted, with the reason for each.

Features are described in full in [USERGUIDE.md](USERGUIDE.md); this document links to them.

## Controls by criterion

### CC6: logical and physical access

| Control | In Quillo | To turn on |
|---|---|---|
| Sessions can be ended: at logout, by an admin, on a password change | Server-side sessions, with an id in the cookie and the data in `_quillo_sessions`; `end_session`, `end_sessions(user)`, `user_sessions(user)` | `sessions({ store: "db", idle: "30m", max_age: "12h" })` ([Sessions that can be ended](USERGUIDE.md#sessions-that-can-be-ended)) |
| Idle and absolute session timeouts | `idle` and `max_age` | As above |
| A new session id at login | `set_session` issues a new id when its `user` changes; `renew_session()` | Name the signed-in person `user` in the session |
| Cookies a sibling site cannot plant | `__Host-` cookie name, HttpOnly, Secure, SameSite=Lax | On by default when the cookie is Secure |
| A second factor | TOTP (RFC 6238) with replay protection, and recovery codes | `crypto.totp_*`, `crypto.recovery_codes` ([Two-step sign-in](USERGUIDE.md#two-step-sign-in)) |
| Passwords stored safely | Argon2id with a random salt; the same timing for unknown users | `crypto.hash_password`, `crypto.verify_password` |
| Brute force limited | Sliding-window rate limits, shared across servers when sessions are in the database | `rate_limit(key, n, per)` ([Rate limits](USERGUIDE.md#rate-limits)) |
| Browsers protected from framing, sniffing and injected script | `X-Frame-Options`, `frame-ancestors`, `Referrer-Policy`, `nosniff` by default; HSTS over HTTPS; a Content-Security-Policy with a nonce per answer | `csp()` ([Security headers](USERGUIDE.md#security-headers)) |
| Output encoding | Values are escaped in HTML; event handler attributes take only written code; URL attributes refuse `javascript:` | Always on |
| Code written by users sandboxed | `run_script` sees only what it is given, has no effects, and is limited in steps, memory and nesting | ([Scripts from users](USERGUIDE.md#scripts-from-users)) |
| Least privilege | A program's functions declare their effects (`with db, http`), checked by `quillo check` | Roles and permissions are the application's: see the CRM's `lib/roles.qwe` for one way |

### CC6.7 and C1: data in transit and at rest

| Control | In Quillo | To turn on |
|---|---|---|
| TLS to database servers | Required by default for any host that is not local; certificates and host names checked | On by default; `sslmode=` in the URL to change it ([Database](USERGUIDE.md#database)) |
| TLS to browsers | Quillo serves HTTP behind a proxy that terminates TLS (limer, nginx, a load balancer) | The operator's proxy |
| Sensitive fields encrypted | AES-256-GCM, with a key id in each value so keys can be rotated | `QUILLO_ENCRYPTION_KEYS`, `crypto.encrypt`, `crypto.decrypt`, `crypto.reencrypt`; `quillo keygen` ([Encrypted fields](USERGUIDE.md#encrypted-fields)) |
| Secrets kept out of the browser | Values read from `env`, files, the database, services or `crypto.decrypt`, and anything computed from them, are never sent in a page's code | Always on |
| Sensitive values kept out of logs and errors | Passwords, tokens, card numbers (by Luhn) and IBANs are masked in error messages, `print` and the request log | Always on; `redact([...])` adds keys ([Sensitive values](USERGUIDE.md#sensitive-values)) |
| Signed calls between services | HMAC with time, target service, call id and body; a call runs once | On for services ([Services](USERGUIDE.md#services)) |
| Incoming webhooks verified | Stripe, GitHub and Shopify signatures, with a time tolerance, compared in constant time | `crypto.verify_webhook` ([Webhook signatures](USERGUIDE.md#webhook-signatures)) |

Disk encryption, key storage (a KMS or secret manager that sets `QUILLO_ENCRYPTION_KEYS`)
and the proxy's TLS settings are the operator's.

### CC7: system operations and monitoring

| Control | In Quillo | To turn on |
|---|---|---|
| An audit trail of who did what | `audit(action, details)`: append-only, each row chained to the one before by SHA-256, written in the same transaction as the change | ([Audit log](USERGUIDE.md#audit-log)) |
| Tampering detected | `audit.verify()` finds a changed, missing or re-hashed row | Run it on a schedule; keep the newest hash it returns somewhere else, which also catches rows cut from the end |
| The audit log cannot be changed through the program | `db.exec` and `db.query` refuse writes to `_quillo_audit*` | Also revoke UPDATE and DELETE in the database (the guide gives the SQL) |
| Request logs an auditor can use | JSON lines with time, request id, user, tenant, method, path, status and duration; a file per day, kept N days | `--log json --log-file F --log-keep N` ([Logs and metrics](USERGUIDE.md#logs-and-metrics)) |
| Metrics and health | Prometheus metrics; a health endpoint; graceful stop | `--metrics` ([Stopping and health](USERGUIDE.md#stopping-and-health)) |

Shipping logs to a collector, alerting on them and keeping them for the period an auditor
asks (often a year) are the operator's. `--log-keep` deletes nothing unless given.

### CC8: change management

| Control | In Quillo |
|---|---|
| Programs checked before they run | `quillo check`: types, effects, SQL built from values, handler attributes, nesting |
| Database changes versioned | Migrations, applied in order and recorded ([Database](USERGUIDE.md#database)) |
| Quillo's own changes tested | CI runs the full test suite, with PostgreSQL, MariaDB and Chrome on the runner, and checks every example |
| Dependencies checked | `cargo deny` in CI for known vulnerabilities, licences and sources (`deny.toml`); Dependabot weekly |
| Vulnerabilities reported privately | [SECURITY.md](SECURITY.md) |

Branch protection, review before merge and separating who writes from who deploys are the
operator's, in their repository and deploy pipeline.

### A1: availability

| Control | In Quillo | To turn on |
|---|---|---|
| Backups | `db.backup(path, { keep })` copies a SQLite database while it is in use | A `schedule` job ([Database](USERGUIDE.md#database)); `pg_dump`/`mysqldump` for servers |
| Restores tested | A backup is one SQLite file that opens as it is | The operator restores on a schedule and records it |
| Overload contained | Request size and header limits, slow-client timeouts, per-client caps on waiting connections and large-file sends, bounded password hashing and image conversion | On by default |
| Several servers | Behind limer, with live updates between them | ([Several servers](USERGUIDE.md#several-servers)) |

### PI1: processing integrity

| Control | In Quillo |
|---|---|
| Exact amounts | `dec`, an exact decimal (`12.50m`); mixing it with a float is an error; seven rounding modes ([Decimals](USERGUIDE.md#decimals)) |
| Amounts in their currency | `money`: ISO 4217 minor units, no adding of two currencies, `allocate` that loses no cent ([Money](USERGUIDE.md#money)) |
| Dates as finance counts them | Business days, holiday calendars, day counts (ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 variants) ([Dates and times](USERGUIDE.md#dates-and-times)) |
| A retried request is applied once | `idempotent(f)`: the work and its stored answer commit together ([Idempotent requests](USERGUIDE.md#idempotent-requests)) |
| Concurrent updates do not lose writes | `db.transaction({ isolation: "serializable", retries })`, retried on conflicts and deadlocks |
| Inputs checked at the boundary | Route patterns, records, and server fn arguments checked all the way in, with unknown fields dropped |

`examples/ledger.qwe` puts these together: double-entry postings that must sum to zero
(checked in code and by the database), idempotent transfers, serializable transactions and
an audit row for each transfer. `tests/ledger.rs` runs it with concurrent, repeated
transfers and checks every invariant afterwards.

## What stays the operator's

Quillo gives the mechanisms above. A SOC 2 report also needs, and Quillo cannot provide:

- policies (information security, access, change management, incident response, vendor
  management, data retention) and evidence that people follow them;
- access reviews of who can reach production, the database and the keys;
- background checks, security training, and onboarding and offboarding;
- the hosting: network rules, disk encryption, the TLS proxy, key management, backups'
  storage and their restore tests;
- monitoring and alerting on the logs and the audit chain, and an on-call rota;
- a risk assessment that includes the accepted risks below.

## How Quillo is checked

- **Security review** (2026-10-26) of the runtime, the browser runtime and the CRM: 1
  critical, 4 high, 12 medium and 16 low findings, all fixed with a regression test each;
  12 informational items, handled as listed below.
- **Fuzzing** of the parser, the checker, HTTP framing, JSON, decimals and dates
  (`fuzz/`).
- **Tests**: the full suite runs in CI on every change, against SQLite, PostgreSQL, MySQL
  and Chrome.
- **Dependencies**: `cargo deny` and Dependabot, as above.

Not yet done: a review by a third party, and running the test suite on Windows and Linux
by hand (CI builds them).

## Accepted risks

Each entry is a known limit that remains after the security review's fixes, why it is
accepted, and what reduces it. Review this list when Quillo changes.

| # | Risk | Why accepted | Mitigation |
|---|---|---|---|
| R1 | Behind a proxy, every client has the proxy's address: per-client limits on large-file sends (8), waiting connections (256) and rate limits keyed by `request.ip` apply to all clients together | Quillo has no trusted-proxy setting and does not read `X-Forwarded-For`, which a client could forge | Limit per client at the proxy |
| R2 | A reader just above 16 KiB/s can hold a large-file send place for up to an hour | Slow but real clients (mobile) must be served | The per-client cap; limits at the proxy |
| R3 | The record of answered service call ids is per process: with a service at several addresses, a captured call can be replayed once to another instance within 5 minutes | Sharing the record would need a store between services | HTTPS or a private network between services |
| R4 | `raw(...)` of script text built outside a `<script>` element is not inspected; inside one, `</` is rewritten as `<\/` even outside strings | `raw` is the program saying "this is markup"; the rewrite affects only code such as `a</b/` | Do not build script text from data |
| R5 | Only the literal form `raw(t(key, vars))` escapes its values; `raw` of a variable holding `t(...)` does not | The checker cannot know where text came from | Documented; write `raw(t(...))` directly |
| R6 | The SQL-injection warning is a heuristic: SQL built in another function, in lists made with `map`, through connections stored in maps or in parameters is not followed | Following all of it needs data-flow analysis | Values are always bound as parameters by the database functions; `sql_name`/`sql_param` for names |
| R7 | CSS in `style` attributes and `<style>` takes data as written: it can restyle a page and load images (beacons), not run script | Styling from data is a feature | Content-Security-Policy `img-src` when it matters |
| R8 | The audit chain cannot show rows removed from its end by someone with database access | A chain proves only what precedes its last hash | Keep the newest hash from `audit.verify()` elsewhere; revoke DELETE in the database |
| R9 | `idempotent` covers the program's own database only: an outside call (a card charge over `http`) inside it cannot be rolled back | No transaction spans another company's API | `{ transaction: false }` keeps every answer, failures included; send the same key to the outside service |
| R10 | Work inside a transaction with `retries` runs again on a conflict | That is how serializable transactions stay correct | `quillo check` warns about `http` inside one; keep outside effects out |
| R11 | On MySQL, a `dec` parameter inside an SQL expression is converted to a double by MySQL | MySQL's own conversion | `cast(? as decimal(p, s))`, documented |
| R12 | Session and rate-limit tables in the database were tested on SQLite, PostgreSQL and MariaDB through the test suite; a database user without CREATE must have the tables made beforehand | Tables are created when first needed | Create them in a migration (the guide lists them) |
| R13 | In the CRM, VAT numbers are not checked against VIES: an order with a reverse-charge VAT number is flagged for review, and the tax is still taken off | VIES is an outside service with its own outages | Orders are flagged and a task is made for each |
| R14 | With `csp()` on, merchants' own inline scripts in CRM pages are blocked | That is what the policy is for | Scripts as files, or turn the policy off for those pages |
