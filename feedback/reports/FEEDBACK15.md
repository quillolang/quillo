# Feedback from running the documentation site under `--watch`

| | |
|---|---|
| App | No app: quillolang.org itself (the website repository, 22 program files, 58 routes, 479 components, using the component kit's `quillo-ui.qwe`, a 4 MB file of 1,643 definitions), edited with `--watch` |
| Quillo | 0.6.0 (9de4ece) |
| Model | Claude (Fable 5.1), the same session that wrote the function reference |
| Tested with | `quillo run --watch` timed from start to `listening`, `sample` (macOS) on the process, the browser source-map test (headless Chrome), the repro under `feedback/run.mjs` |

What worked well: the site itself is fast. A plain `quillo run` of the whole thing, 2,217
functions and 479 components plus the 315 reference pages read and parsed at load, is up
in 0.6 s, and a reference page renders in 15 ms. Only the development mode was slow, and
only because of one design choice in the source maps.

## Seen again

None.

## New issues

### Q-097. Under `--watch`, every function compiled from a file carries the whole file's text in its source map

| Kind | Detected by | Attempts |
|---|---|---|
| perf | timing `--watch` against a plain run, then `sample` on the process | 1 |

Editing any file of the site took about 8 s before the page refreshed. The site starts in
0.6 s without `--watch` and took 7.3 s with it, so the time was in development mode, not
in the program. A sample of the process put 99% of the start in `client::dev::bundle`:
44% in serde_json writing a map to a string, 33% in `ws::base64`, the rest in the string
growing. Memory peaked at 870 MB during a reload and 176 MB after.

The cause is in `with_map` (src/client.rs): the map of each compiled function has
`sourcesContent` set to the whole text of the `.qwe` file it came from. The site compiles
hundreds of definitions from the kit's `quillo-ui.qwe`, 4 MB, so that file was written as
JSON and base64-encoded hundreds of times over on every reload: several hundred megabytes
of text for a program whose own source is 5 MB.

Measured, with the site:

| | Before | After |
|---|---|---|
| `quillo run --watch` to `listening` | 7.3 s | 0.5 s |
| A reload after editing docs.qwe (three in a row) | 17 s, 31 s, 64 s | 1.1 s, 1.0 s, 0.9 s |
| Resident memory after start | 870 MB peak | 176 MB |
| Without `--watch` | 0.6 s | 0.6 s |

The reloads were slower than the start, and unevenly so, because the previous version's
bundle files stay in memory until the cache of 256 bundles is cleared, so each reload
grew the process by hundreds of megabytes more.

The repro ([repros/Q-097-dev-map-text-per-piece.qwe](../repros/Q-097-dev-map-text-per-piece.qwe))
loads a page's dev scripts and counts the maps that name its file and the maps that carry
its text: three maps, three copies before, three maps and one copy after.

```quillo
fn double(n: int) = n * 2
fn label(n: int) = `count ${n}`

component Counter() {
    state count = 0
    <div>
        <p>{label(count)}</p>
        <button @click={() => { count = double(count) + 1 }}>Go</button>
    </div>
}

route GET / = <html><body><Counter/></body></html>
```

**Fix, on branch dev-maps-once:** a file's text goes into the map of the first piece
compiled from it only; the other pieces name the file (`sources`) and carry no copy
(`sourcesContent: [null]`). The browser shows the file through the first map and the
editor sets breakpoints through the `file://` name as before, so the layout, one script per
function, and the browser test's expectations are unchanged but for the test now accepting
the text in any one of a file's maps. A larger change was considered and set aside: pointing
`sources` at an address the server serves instead of embedding text at all would have broken
breakpoints set from an editor, which match the `file://` names.

### Q-098. A reload under `--watch` redoes work that has not changed: the program compiled for the browser twice, every file parsed again, the whole program printed as text to look for one word, and a poll of 250 ms before any of it

| Kind | Detected by | Attempts |
|---|---|---|
| perf | `sample` on the process during a reload, after Q-097 | 1 |

With Q-097 fixed, the site loaded in 0.5 s under `--watch`, and a reload showed after
about 0.5 s. A sample of the reload put the time in four places, none of them the edit:

- `build_client` compiled the whole program for the browser twice, once for the
  whole-program bundle and once for the shared runtime file (`client::bundle` and
  `client::runtime_file` each called `compiled(it, None)`): 258 of the load's 390 ms.
- Every file was parsed again on every reload, the kit's 4 MB with them, though only
  the edited file had changed: about 130 ms.
- `db::uses_atomic` decided whether the program names `db.atomic` by printing the
  whole program as Debug text and searching it for the word: 17 ms per reload.
- The watcher polled every 250 ms and waited 50 ms more after a change: up to 300 ms
  before the reload began.

Measured on the site, reload after an edit of docs.qwe, wall clock from the save:

| | Before | After |
|---|---|---|
| Cold start under `--watch` to `listening` | 0.5 s | 0.19 s |
| Save to `reloaded`, five in a row | 0.5 s each | 0.12 to 0.18 s |
| Of which the reload's own work | 213 ms | about 100 ms |

No repro: the issue is timings, measured as above.

Seen while verifying, not caused here: tests/udp.rs fails now and then on a connection
reset by the server on a test's first request after it starts (`a_program_sends_text_bytes_and_values`
and three others), 3 runs of 12 on main (9de4ece) and 1 of 12 on this branch, the file run
alone each time; tests/services.rs and tests/native.rs showed one such reset each under
the full suite. Those servers run without `--watch` or `--reload`, so the watcher is not
involved. Worth a look at how the first connection after `listening` is handled.

**Fix, on branch dev-maps-once:**

- `client::bundle_and_runtime` compiles once and makes both the bundle and the runtime
  file from it; `build_client` calls that.
- Parsed programs are kept by file with a hash of the text they came from
  (`parse_cached` in src/interp.rs): a reload parses the files that changed, and reads
  the others' parses back.
- `uses_atomic` is a flag noted as each file is read (`.atomic` in its text).
- In development the whole program's bundle (`/_quillo/quillo.js`, or a page's
  bundle no longer kept) is made on a thread of its own once the program is up, not
  before it answers: each page compiles its own components anyway. A component that
  cannot run in the browser is reported in the log right after the load instead of as
  the load's error, and in the browser's console of a page that needs it; `quillo
  check` says so as before, and without `--watch` the load fails as before.
- The watcher polls every 50 ms and waits 30 ms after a change.

The site also changed on its side: the reference pages' blocks are parsed when a page
is asked for, not all 315 at load, which took 0.1 s of the program's top level.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Timing the reload by counting log lines that were already there | A grep that matched the start-up lines | 23 s and 34 s reported for edits that triggered nothing; a guide page is read at start, not watched, so editing one never reloads | 1 run |
| `sample` on the pid from `pgrep -f` | The first match was the shell wrapping the server | A profile of zsh | 1 run |
| A `sed -E` with `*?` to shorten Rust symbol names | PCRE habit | BSD sed refuses the lazy quantifier | 2 runs |
| Reading a reload's profile as "the compiler is slow" | The compiler's frames were the heaviest | It was called twice; one call was thrown away | 1 read |

## Limitations

- Editing a file the program only reads (`read_file`, a guide page or a reference page
  of the site) does not reload under `--watch`: the watcher follows the program's files
  and the files it served, not what `read_file` opened. The site's docs pages need a
  restart, or a touch of a program file, to show an edit. Not an issue filed: it is how
  `--watch` is defined, but worth a line in the guide.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A program with no components under `--watch` | no dev bundle is made; start time as without `--watch` |
| A piece with no source file (a constant made from its definition) | no map, as before (Q-047) |
| Two functions from one file, the second compiled first | the text goes with whichever piece comes first in the loader's order; the browser loads them in that order |
| The browser source-map test (breakpoint on `count += 1` through the maps, locals by their names) | passes with the text in the first map only |
