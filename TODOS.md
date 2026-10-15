# TODOs

Pending work on Quillo, roughly in the order it would pay off. Security
hardening (request limits, cross-site checks, connection caps, error messages,
header checks, cluster announcement checks) is being done in a separate audit
and is not listed here.

## Reactivity in the browser

Today a component updates as a whole: assigning `~x` re-runs the component's
function, diffs the virtual DOM and patches what changed; every child
component updates with it.

- [ ] **Skip children whose props did not change.** Values are immutable, so
      "same props" is an identity check per prop (`VComp` in `patch`,
      `src/client/runtime.js`). Callbacks written inline are new closures on
      each render and will still count as changed.
- [ ] **Skip assignments that change nothing.** `Inst.set` queues an update
      even when the new value is the old one.
- [ ] **Fine-grained updates (later, if profiling asks for it).** The compiler
      knows which template expressions read which `~state`; it could build the
      DOM once and update only the nodes that depend on what changed (as
      Svelte and Solid do), replacing the virtual DOM. Large change to
      `src/client.rs` and the runtime; the differential tests against Node
      (`tests/client.rs`) are the safety net.

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

## Several servers behind limer

- [ ] **Leave the pool on shutdown.** On SIGTERM/Ctrl-C: `POST /drain`, finish
      in-flight requests, `POST /deregister`, exit. Today a stopped server
      stays in the pool until its 15-second TTL lapses.
- [ ] **A health endpoint** (`/_quillo/health`) for limer's `health_uri`, so a
      server is admitted only once it has loaded its program.
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
- [ ] **A faster HTTP layer.** With the cache, tiny_http's per-connection
      threads and queue are the limit (plain text: about 95,000/s).
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
- [ ] **Per-page bundles.** Each page could get only the components it uses;
      today `/_quillo/quillo.js` carries every component of the program.
- [ ] **Shorter two-way binding** for state, such as `<input ~value={name}/>`
      next to `bind:value`, if it reads better in practice.
- [ ] **JavaScript-truthiness warnings beyond known types.** The checker warns
      about `||`, `&&`, `!`, `if` and `? :` only where a value is known to be
      a string or a number. Callbacks such as `filter((x) => x.name)` and
      untyped parameters are not covered.
