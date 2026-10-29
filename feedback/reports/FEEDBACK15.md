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

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Timing the reload by counting log lines that were already there | A grep that matched the start-up lines | 23 s and 34 s reported for edits that triggered nothing; a guide page is read at start, not watched, so editing one never reloads | 1 run |
| `sample` on the pid from `pgrep -f` | The first match was the shell wrapping the server | A profile of zsh | 1 run |
| A `sed -E` with `*?` to shorten Rust symbol names | PCRE habit | BSD sed refuses the lazy quantifier | 2 runs |

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
