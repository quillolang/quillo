# TODOs

Pending work on Quillo, roughly in the order it would pay off. Security
hardening (request limits, cross-site checks, connection caps, error messages,
header checks, cluster announcement checks) is being done in a separate audit
and is not listed here.

## Production

Known limits of what is there, from the security review and the soak test:

- [ ] **Live topics are not checked against the tenant.** `/_quillo/live` takes
      the topic names a page sends, so a page on one tenant could ask to be woken
      by another's changes. It learns only when they happen, never what changed.
      Signing the topics a loader returns would close it.
- [ ] **Schema per tenant uses a pool per tenant.** At most 16 PostgreSQL tenants
      keep connections open (four each at most); one shared pool that sets the
      `search_path` per transaction would serve any number.
- [ ] **A check that a tenant exists.** Requests for a made-up tenant make nothing,
      but `tenant()` still names it; `tenant_from(rule, { known: (name) => ... })`
      would let routes treat it as no tenant.
- [ ] **Large text files go uncompressed.** Files over 1 MiB are sent from disk as
      they are; compressing them as they are sent, or keeping a `.gz` beside them,
      would shrink large JSON and CSV.
- [ ] **Sessions are signed, not encrypted**, so their contents are readable in
      the browser (documented). Encrypting them would allow keeping more in them.
- [ ] **Jobs of mounted programs do not run**: only the main program's
      `schedule` calls are seen by the scheduler.

## Native apps

`quillo app` shows a program in a native window on the desktop (see
`src/desktop.rs`). Next, in order:

- [ ] **Native features as builtins**, for what a page cannot do: file open
      and save dialogs, notifications, the app's data folder (so SQLite files
      go there rather than next to the program), window title and menus.
- [ ] **Packaging: `quillo bundle`**, producing a `.app`, `.msi` or `.AppImage`
      with the program inside, and an icon.
- [ ] **Mobile.** The interpreter built as a library for iOS and Android,
      hosted by a small Xcode and Gradle project in the platform's web view.
      Check first that every dependency cross-compiles.
- [ ] **Windows and Linux tried by hand.** CI builds them; only macOS has been
      run.

## Reactivity in the browser

Fine-grained: a component's (and a view's) code runs once; `state` is a
signal, a `let` that reads what changes a memo, a statement that does an
effect, and each hole in the markup an effect on its own node. Props are
getters, `for` in markup keeps a row per key (or place) with its item as a
signal, and `if` / `match` make a branch again only when another is chosen.
Assigning the value a state already has does nothing.

- [ ] **Faster row creation.** Making 1,000 rows takes about 2.1 ms of
      script against Solid's 1.4 (bench/README.md): a scope, an item signal,
      a template instance and its hole functions for each row, and the
      template walked generically (`wire`); code made per template, as the
      compiler once made for blocks, would walk it straight.
- [ ] **Bodies made again whole.** A component body that can `return` early
      (or use `?`), or assigns its own variables as it runs, is one region
      made again when what it read changes (its state kept). The compiler
      could say so, or split such bodies too.
- [ ] **Inputs that the state did not change.** An input's `value={x}` is
      put back after its own events, but not when another element's handler
      sets `x` to the value it already had (nothing changed, so nothing
      runs).
- [ ] **Cheaper calls in compiled code.** Every function call checks its
      arguments and catches `return`; building 1,000 rows in Quillo code costs
      several times what plain JavaScript does.

## Live data

- [ ] **Changes made outside Quillo.** Only writes made through a Quillo server
      wake pages; a row changed with `psql` or by another program does not.
      For PostgreSQL: `LISTEN/NOTIFY` from triggers, or logical replication.
- [ ] **Missed announcements between servers.** Broadcasts through limer are
      delivered once; a server that is unreachable at that moment never learns
      of the change, and its pages show it only on the next change or reload.
      Options: servers exchange their latest announcement number and re-ask
      for gaps, or pages reload their live data when their socket reconnects.
- [ ] **Several processes on one SQLite file** (one machine, no limer): a small
      changes table that each process polls.
- [ ] **Round-robin reconnects.** A page whose live socket lands on a different
      server reloads all its live data once. Fine with `sticky_cookie`; worth
      measuring without it.

## Tests

- [ ] **Ports taken by another test.** `tests/cluster.rs`, `tests/browser.rs` and
      `tests/services.rs` pick a free port and release it before their server
      takes it, so a test running alongside can get it first, and the test then
      talks to the wrong server. `tests/streams.rs` waits for its own server to
      say it is listening and tries another port if it could not; the others
      should too.

## Several servers behind limer

- [ ] **Run `tests/cluster.rs` in CI.** It is skipped without a limer binary;
      CI would need to build limer (`$LIMER`).
- [ ] **An example**: a Limerfile plus a `.qwe` app on PostgreSQL, run as two
      servers, in `examples/`.

## Server rendering speed

The 50-row benchmark page renders at about 26,000 pages/s on one core
(Svelte 5: about 24,800), and at about 84,000 when its views come from the
view cache. Next, by expected gain:

- [ ] **Static markup in components.** Components are never cached (their
      state and live version are per request); the parts of their HTML that
      never change could be rendered once.
- [ ] **Variables resolved at load time.** Names are still looked up by text
      at each use; resolving them to (depth, slot) once would remove most
      lookup cost.
- [ ] **Opt-in caching for views that read the database**, with a time limit,
      since rows changed outside Quillo are not seen.

## Examples

- [ ] **Shop (`examples/spa.qwe`): products in a database**, so stock survives
      restarts and is shared when the shop runs on several servers.
- [ ] **Shop: a server-side cart.** The cart lives in the page, so a reload
      empties it, and items left in an abandoned cart stay reserved; a cart
      kept on the server (per session) with reservations that expire.
- [ ] **Todo (`examples/todo.qwe`): no full reloads.** It uses plain forms, so
      every action reloads the page. Either progressively enhanced forms
      (submitted with `fetch`, page patched in place, still working without
      JavaScript) or a component version.

## Language and tooling

- [ ] **Enhanced forms** as a feature: `<form enhance>` (or similar) submitting
      with `fetch` and swapping in the response, for apps that are mostly views.
- [ ] **Shorter two-way binding** for state, next to `bind:value`, if one reads
      better in practice.
- [ ] **Connections in components, found by `quillo check`.** A component that
      uses a `db.connect` connection (or anything read from the database) stops
      the program from loading, with a clear message, but `quillo check` does not
      report it beforehand.
- [ ] **One interface for database drivers.** Transactions share one path for
      PostgreSQL and MySQL; the rest (queries, pools, migrations, tenants) is still
      written per driver in `src/db.rs`, so a new one touches several places.
- [ ] **JavaScript-truthiness warnings beyond known types.** The checker warns
      about `||`, `&&`, `!`, `if` and `? :` only where a value is known to be
      a string or a number. Callbacks such as `filter((x) => x.name)` and
      untyped parameters are not covered.

## Banking and finance

What a bank's or a fintech's review would stop on first. Money is only `int`
and `float` today: a PostgreSQL `NUMERIC` or MySQL `DECIMAL` column comes back
as a float (`src/pg.rs` `numeric`, `src/my.rs`), so sums of prices drift by
fractions of a cent, and ints past 2^53 are rounded in the browser.

First, since their own review would find them:

- [ ] **The open findings of `SECURITY_REVIEW.md`.** C1 (`run_script` reaches
      the program's globals through `"x".env`) and H1 (`dechunk`'s
      `body.len() + size` overflows) are still in the code. Then H2 to H4 and
      the M items, in the order the review gives.

Numbers:

- [ ] **A decimal type, `dec`.** Exact base-10 numbers: a literal, `+ - *`
      exact, `/` and `round(x, 2, "half_even")` with a stated rounding mode,
      `dec` and `int` compared exactly, and `dec` with `float` an error rather
      than a silent float. An i128 with a scale (28 digits, as C#'s decimal)
      covers money without a big-number library.
- [ ] **Decimals from and to databases.** `NUMERIC` and `DECIMAL` come back as
      `dec` and are written back exactly; SQLite keeps them as text.
- [ ] **Decimals in JSON and in the browser.** JSON gives them as strings
      (`"12.50"`); route and record patterns take `dec` from strings and
      numbers. The browser runtime needs its own decimal (over `BigInt`), and
      ints past 2^53 must reach a page whole, not rounded.
- [ ] **The checker and the JIT.** `dec` as a type in `quillo check`, with a
      warning where a float meets one; compiled numeric functions leave `dec`
      to the interpreter.
- [ ] **Money.** An amount with its currency and the currency's minor units
      (ISO 4217: JPY 0, USD 2, BHD 3). Adding two currencies is an error,
      `money.allocate(total, [1, 1, 1])` splits without losing a cent, and
      amounts are formatted by locale (`1.234,50 €`) through translations.

Dates:

- [ ] **Dates and times.** Only `now()` (an int) exists. Finance needs dates
      without times (value dates), time zones, adding months (31 January plus
      a month), day counts (30/360, ACT/365) and business days with a holiday
      calendar.

Correct under retries and load:

- [ ] **Idempotency keys.** A route or server fn that takes an
      `Idempotency-Key` and answers a retry with the first answer, kept in a
      Quillo table, so a payment retried after a timeout is made once.
- [ ] **Isolation levels.** `db.transaction` uses the database's default;
      `db.transaction({ isolation: "serializable", retries: 3 }, ...)` would
      retry on serialization failures. Document `select ... for update` for
      balances.
- [ ] **A ledger example.** Double-entry accounts and postings in
      `examples/ledger.qwe`, and a test that runs transfers concurrently and
      checks that every balance still sums to zero.

Compliance:

- [ ] **An audit log.** Who did what, when and from where; append-only, each
      row hashing the one before, written in the same transaction as the
      change.
- [ ] **Encrypted fields.** `crypto.encrypt` and `crypto.decrypt` (AES-GCM),
      with keys from the environment or a KMS; with encrypted sessions (see
      Production).
- [ ] **Account and card numbers kept out of logs.** A way to mark fields so
      logs, error pages and metrics mask them.
- [ ] **Webhook signatures.** `crypto.hmac` exists; a helper that checks a
      timestamped signature (Stripe's form and similar) with a tolerance, in
      constant time.

## SOC 2

SOC 2 is an audit of an organisation, not of a language: what Quillo can do is
give the apps built with it (the CRM first) the controls an auditor looks for,
and keep its own development to the standard an auditor of those apps will ask
about. Several items above count here too: the audit log, encrypted fields,
masked logs, and the open security review.

In the runtime:

- [ ] **Sessions that can be ended.** Signed cookies cannot be revoked: a
      session id kept in a Quillo table, so that logging out, an admin, or a
      password change ends it everywhere; an idle timeout beside the absolute
      one; a new id on login; `__Host-` cookies (review L3).
- [ ] **A second factor.** `crypto.totp` and its check (RFC 6238, with a
      window), recovery codes, and the CRM asking for it.
- [ ] **Rate limits.** `rate_limit(key, n, per)` for routes and server fns,
      shared across servers through limer, and lockout after repeated failed
      logins.
- [ ] **Security headers by default.** `Strict-Transport-Security` over HTTPS,
      `Referrer-Policy`, `frame-ancestors` (or `X-Frame-Options`), and a
      Content-Security-Policy with a nonce per response (review L10).
- [ ] **TLS to databases by default** for hosts other than localhost (review L12).
- [ ] **Logs an auditor can use.** JSON lines with time, request id, user,
      tenant and outcome; sent to a file or a collector; kept for a set time.
- [ ] **Backups.** `db.backup(path)` for SQLite (its online backup API), on a
      schedule, and a documented restore that is tested.
- [ ] **The review's Low findings** (L1 to L16), each fixed or written down as
      accepted with a reason.

For the project:

- [ ] **`SECURITY.md`**: how to report a vulnerability, and what happens then.
- [ ] **Dependencies checked in CI** (`cargo deny`: advisories, licences,
      sources), and Dependabot for Cargo, npm and GitHub Actions.
- [ ] **A controls guide**: which of the Trust Services Criteria Quillo's
      features help an app meet (access, change management, monitoring,
      encryption, availability), and what stays the operator's to do.
