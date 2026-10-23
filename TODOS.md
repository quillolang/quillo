# TODOs

Pending work on Quillo, roughly in the order it would pay off. Security
hardening (request limits, cross-site checks, connection caps, error messages,
header checks, cluster announcement checks) is being done in a separate audit
and is not listed here.

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

Assigning `~x` re-runs the component's function. Markup of a fixed shape is
compiled into blocks (a cloned template with holes for what can change), so
an update compares hole values rather than whole trees; keyed lists move only
the rows that must move; and the rows of a keyed `for` loop are kept from one
render to the next, with only the holes that read what changed recomputed.
Child components still update whenever their parent does.

- [ ] **Skip children whose props did not change.** Values are immutable, so
      "same props" is an identity check per prop (`VComp` in `patch`,
      `src/client/runtime.js`). Callbacks written inline are new closures on
      each render and will still count as changed.
- [ ] **Skip assignments that change nothing.** `Inst.set` queues an update
      even when the new value is the old one.
- [ ] **Keep more than keyed loop rows.** Rows without a `key`, and loops in
      views (which have no component to remember them in), are made again on
      each render. So is markup outside loops; its holes could be recomputed
      only when what they read changed, like kept rows'.
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
