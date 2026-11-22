# Feedback from moving a 3D animation studio onto Quillo: a three.js engine, a voice bridge, video export and MCP tools

| | |
|---|---|
| App | Candy Studio (`~/dev/studio`), an editor for children's animated stories on three.js, moved off a Vite dev server at the user's request, 2026-11-22: "lets re-write using quillo", then "and use mcp if possible", then "submit all challenges to quillo-app1 for feedback". Quillo holds the server, the editor's panels and timeline (12 components), the voice bridge to a local text-to-speech server, a story library, autosaved drafts and 8 MCP tools: 969 lines of `.qwe` in 11 files, 106 lines of Rust in 2 plugin files. The 3D engine stays JavaScript (`src/core.js`, 1,252 lines, ES module) reached with `client module` and `js()` |
| Quillo | `quillo 0.8.10 (61997ab, 2026-11-22)` on the PATH (= tag v0.8.10). Every repro below was run on that binary |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check` after every change; `quillo run --watch`; headless Chrome over CDP (selecting, editing, dragging on the timeline, opening a story through `ask`, choosing a file in a file input, saving); `quillo mcp` driven over stdio; 30+ one-minute 1080p videos rendered through the program's pages. Repros were kept in a scratch folder and are written out in this report |

Numbers Q-1340 to Q-1348, from the block Q-1340 to Q-1359, agreed with New Quillo Coder (FEEDBACK84 holds Q-1320 to Q-1339). This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed.

What worked well (measured):

- **A big JavaScript engine fits cleanly.** `client module /src/core.js use createStudio`, with an import map in the page (`<script type="importmap">{raw(IMPORT_MAP)}</script>`) pointing `three` at `node_modules`, loaded the engine and three.js 0.170 with no bundler; the components started only once the module was in, as the guide says. The engine calls back into Quillo through a function handed to `js("CandyStudio.subscribe", () => { snap = ... })`, and the panels follow it.
- **`quillo check` paid for itself.** It found 3 effect mistakes (a plugin needing `net` called from `with fs` functions, `uuid()` without `random`) and 3 `http.*` calls without `public_only`, all before anything ran. The final program: `ok (10 routes, 42 functions, 12 components, in 11 files)`.
- **The browser seam held up under real use.** `e.currentTarget.setPointerCapture(e.pointerId)` and `getBoundingClientRect()` in handlers for dragging blocks on a timeline; `js_new("ResizeObserver", () => ...)` with the observer disconnected by the function `@mount` returns; a file input's `f.text()` awaited inside `onchange`; `ask` and `notify` styled by the page's own `q-` classes.
- **Rust plugins were painless.** A 60-line `.rs` file was built on first use, rebuilt under `--watch` in seconds, and ran ffmpeg and Rubber Band for each voice line: a new child's-voice clip in 0.17 s, then served by `file(path, { cache: "1y" })` in 0.4 ms.
- **MCP was the best surprise.** Eight `fn mcp` tools with typed results came out as JSON Schema with `structuredContent`; a tool that renders a one-minute 1080p video (through a plugin, 15-minute timeout) answered in 52 s; a broken story sent to `story_check` came back `{ ok: false, issues: ["Puff: 'moonwalk' isn't a two-legged action"] }`.

## New issues

### Q-1340. `files()` refuses a folder reached through a link without a word, even with `--allow-path`, while `file()` and `use` go through it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `curl` on the running program, then the log | 3 |

Two apps were to share one engine folder, linked into each (`engine -> ../shared`). `use "engine/lib.qwe"` follows the link. `file("engine/web/x.js")` refuses it, says why in the log, and serves it once `--allow-path ../shared` is given. `files("engine/web", path)` answers 404 both times and logs nothing but the 404, so the developer cannot tell a refused link from a missing file, and the option the other refusal names does not help.

```quillo
// app/main.qwe, with app/engine -> ../shared (shared/lib.qwe, shared/web/x.js)
use "engine/lib.qwe"
route GET / = hello()                                  // 200 "from shared"
route GET /e/*path = files("engine/web", path)          // /e/x.js: 404, nothing logged
route GET /one = file("engine/web/x.js")                // 404, logged: "... leads out of the program's folder through a link ... `--allow-path FOLDER` lets the program reach a folder outside"
```

| | no option | `--allow-path ../shared` |
|---|---|---|
| `use "engine/lib.qwe"` | follows | follows |
| `file("engine/web/x.js")` | 404, reason logged | 200 |
| `files("engine/web", "x.js")` | 404, nothing logged | 404, nothing logged |
| `read_file("../shared/lib.qwe")` | error value | the text |

| Impact | |
|---|---|
| **Functionality** | Shared static files cannot be served from a linked folder at all; the app had to serve its engine's scripts through a Rust function that reads them |
| **User** | A developer sees 404s for files that are there, with no log line to explain them |
| **Requests** | No change |
| **Security** | No change: refusing links out of the folder is Q-475's and Q-701's protection and should stay. The ask is that `files()` (a) logs why, as `file()` does, and (b) honours `--allow-path` as `file()` and `read_file` do |

**Suggestion:** route `files()`'s link check through the same code as `file()`'s: the same log line, and the same `--allow-path` exception.

### Q-1341. `read_file`'s refusal of a path outside the folder does not name `--allow-path`

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | run-time error value | 2 (then a Rust plugin was written instead) |

The voice bridge reads a key from the speech server's own `.env`, beside the program (`../tuno/.env`). `read_file("../tuno/.env")` gave `read_file: path must stay inside the program's folder: ../tuno/.env`. Nothing said that `--allow-path DIR` exists (Q-357 added it), so the app grew a Rust function to read the file. `file()`'s log line for the same situation does name the option (Q-1340).

```quillo
echo 'read_file("../tuno/.env").error' | quillo repl
// "read_file: path must stay inside the program's folder: ../tuno/.env"
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | The developer works around a limit that has a switch |
| **Requests** | No change |
| **Security** | No change; arguably better, since the option makes the reach explicit in the start command instead of hiding it in Rust |

**Suggestion:** end the message with the way out, as `file()`'s does: "... `--allow-path ../tuno` (on `quillo run` or `quillo mcp`) lets the program read it". Same for `write_file` and `delete_file`.

### Q-1342. A binary answer from another server cannot be kept: `http.*` bodies are text, and nothing saves one to a file

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the guide, then measuring | 1 |

The bridge must hand the page MP3 clips that the speech server makes. `http.get` gives `body` as text with bytes that are not UTF-8 replaced (as the guide says), so the clip is lost: a 2,176-byte PNG came back with `content-length: "2176"` and a 2,066-character `body`. Files are the guide's way for bytes, but no builtin writes an answer's bytes to one. The app runs `curl -o` through its plugin.

```quillo
let r = http.get("http://127.0.0.1:8080/src/brand/logo-32.png", {}, { public_only: false })
[r.status, r.headers["content-length"], len(r.body)]      // [200, "2176", 2066]
```

| Impact | |
|---|---|
| **Functionality** | A proxy or cache for images, audio or archives needs Rust (or a program started from Rust) |
| **User** | No change once worked around |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** an option on the `http.*` calls that writes the body to a file instead of reading it, `http.get(url, headers, { save: "cache/a.mp3" })`, held to the folder rules of `write_file` (and `--allow-path`), giving `{ status, ok, headers, file, bytes }`. That covers proxies and caches without a bytes type.

### Q-1343. No way to run a program (ffmpeg, a converter, node) without writing Rust

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the guide | 1 |

Making a child's voice runs `ffprobe`, `ffmpeg` and `rubberband` on each clip; the MCP tools run three Node scripts (a checker, a music renderer and the video exporter). The catalog's `ffmpeg_cli` covers ffmpeg only. The app wrote a 60-line `run_tool(program, args, cwd, timeout_ms, env)` in Rust, limited to five named programs and never through a shell. It works well, but every Quillo program that shells out will write the same file, each with its own mistakes (a shell, output read after `wait` and a full pipe, no timeout).

| Impact | |
|---|---|
| **Functionality** | Media pipelines, converters and existing scripts need Rust |
| **User** | No change |
| **Requests** | No change |
| **Security** | A plugin that starts programs runs under whatever effects line its author writes; FEEDBACK83 already suggests a `run` effect for this |

**Suggestion:** a catalog plugin (or builtin) `run(program, args, { cwd, timeout, env })` that takes an argument list, never a shell, reads stdout and stderr together, kills on timeout, and is held by a `run` effect and a policy's list of allowed programs (FEEDBACK83's suggestion; it would cover `ffmpeg_cli`'s needs too).

### Q-1344. `//! effects:` is one line for a whole plugin file, so one function that reaches the network makes every function in it need `net`

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` | 1 (the message was clear) |

The plugin had `run_tool` (starts programs: `fs, net, env, time`) next to small file helpers (`fs` only). A helper called from a `with fs` function was refused: `` `list_files` needs the env, time, net effects, but `all_stories` is declared `with fs` ``. The fix was to move the helpers into a second `.rs` file with its own header. The message was good; the cost is files split by effect rather than by subject.

```rust
// fx.rs
//! effects: fs, net
/// only reads a file
pub fn size_of(path: &str) -> i64 { std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(-1) }
```

```quillo
import fx.rs use size_of
fn file_size() -> int with fs = size_of("fx.rs")
// error: `size_of` needs the net effect, but `file_size` is declared `with fs`
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | Plugins split into files by effect |
| **Requests** | No change |
| **Security** | Per-function lines would let a file's harmless helpers be called from narrow `with` functions, which helps least privilege |

**Suggestion:** let a `///` line on a `pub fn` narrow the file's effects for that function (`/// effects: fs`), never widen them; keep the file line as the default and the upper bound.

### Q-1345. `"""` gives "expected end of statement, found a string", with no word about writing text over several lines

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` (the editor showed it at once) | 1 |

An MCP tool returns a page of guidance written as a long text. The habit from Python and others is `"""`. Quillo parses `""` as an empty string and then a stray `"`, and says only `expected end of statement, found a string`. A backtick string spans lines and was the fix; the message could have said so, as other habits get advice (Q-359).

```quillo
fn guide() -> str = """
line one
"""
// triple.qwe:1: error: expected end of statement, found a string
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A puzzling error for a common habit |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** when `""` is followed at once by `"`, say: "`\"\"\"` is not a string here: text over several lines is a backtick string, `` `line one ... `  `` (`${` interpolates)".

### Q-1346. The MCP door's refusal of a request without `_meta` reads as a statement, and the guide has no raw message to copy

| Kind | Detected by | Attempts |
|---|---|---|
| docs, loud | stdio by hand | 2 |

Driving `quillo mcp` over stdio to test the tools, the first `tools/call` (and `server/discover`) without `_meta` gave `-32602 Invalid params: `_meta` carries io.modelcontextprotocol/protocolVersion and io.modelcontextprotocol/clientCapabilities`. "Carries" reads as describing what was sent; it means "must carry". The guide describes the 2026-07-28 revision and `quillo docs --mcp` prints what a client is told, but shows no request, so there is nothing to copy for a test or a script. (An `initialize` from an older client gets a clear `-32022 Unsupported protocol version: this server speaks MCP 2026-07-28 only`.)

```sh
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ping","arguments":{}}}' | quillo mcp m.qwe
# {"error":{"code":-32602,"message":"Invalid params: `_meta` carries io.modelcontextprotocol/protocolVersion and io.modelcontextprotocol/clientCapabilities"}}
# works with: "params":{..., "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}
```

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A developer testing tools by hand, or from a script or CI, cannot tell what to add |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** "Invalid params: `_meta` must carry `io.modelcontextprotocol/protocolVersion` (\"2026-07-28\") and `io.modelcontextprotocol/clientCapabilities` ({})", and a raw `tools/call` line in the guide's "Tools for AI clients", as the one above.

### Q-1347. `quillo mcp` serves no routes, so a tool that uses the program's own pages needs a second `quillo run`

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the guide, then a tool's error | 1 |

The `story_video` tool renders a video by driving the program's editor page in headless Chrome. Under `quillo mcp` (how a local AI client starts the tools, from `.mcp.json`) routes are not served, so the tool checks for a separate `quillo run` and says to start one. Over HTTP, `/_quillo/mcp` is served by `quillo run`, but a local CLI client then needs a signed-in session and an `mcp_token()`, which a single-user local tool has no use for.

| Impact | |
|---|---|
| **Functionality** | Tools that need the program's pages, files served by it, or its own HTTP API need two processes |
| **User** | One more thing to start before a tool works |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** `quillo mcp main.qwe --serve [--port N]`: the stdio door as now, with the routes served as `quillo run` serves them, one process, the stdio caller being the person who started it.

### Q-1348. A host read from `env()` at the top of the program is warned of as one "a visitor can choose"

| Kind | Detected by | Attempts |
|---|---|---|
| docs, small | `quillo check` | 1 |

`let TUNO = env("TUNO_URL", "http://127.0.0.1:8940")` at the top, then `http.get(\`${TUNO}/health\`)`, gave "the host `http.get` goes to comes from a value: if a visitor can choose it, they can reach this server's own network". The value is the operator's setting, never a visitor's; the fix (`public_only: false`) is right but the wording sends one looking for a visitor's input that is not there.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | 3 warnings to silence by hand on a trusted setting |
| **Requests** | No change |
| **Security** | No change if the warning stays; the ask is only the wording |

**Suggestion:** when the host's value comes only from `env()` and literals, say "from `TUNO_URL`, the operator's setting: say `public_only: false` if it is meant to reach this machine" rather than speaking of a visitor.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `read_file("../tuno/.env")` | the Vite server read it | refused, no way out named (Q-1341) | a Rust function, never needed |
| one `.rs` file for running programs and file helpers | one subject, one file | 3 effect errors from `quillo check` (Q-1344) | a second file |
| `uuid()` in `with fs, net, env, time` functions | forgot `random` | `quillo check` said so at once | 1 check |
| `"""` for a long guide text | Python habit | parse error with no hint (Q-1345) | 1 check |
| raw MCP calls without `_meta` | MCP clients I know send `initialize` | -32602 "carries ..." (Q-1346) | 2 tries |
| `files("engine/web", path)` through a link | `use` had followed the same link | silent 404 (Q-1340) | 3 tries, then a plugin route |
| `quillo run ... &` inside a shell that was itself started in the background | my habit, not Quillo's | the server kept running after its shell ended; harmless | none |
| headless test tabs on the same server as the person using it | testing as a person would | the tests' autosave (an `every()` that saves the working copy) replaced the person's draft | an app design lesson, not Quillo's: per-browser drafts |

## Limitations

- **No bytes and no saving an answer to a file** (Q-1342): `curl -o` through the plugin.
- **No starting programs** (Q-1343): `run_tool` in Rust, five programs named, argument lists only.
- **`quillo mcp` serves no routes** (Q-1347): the tool asks for `quillo run` beside it.
- **`read_file` stays in the folder** unless `--allow-path` (Q-1341): a Rust function reads the one value needed.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `client module /src/core.js use createStudio` with a page import map for `three` and `three/examples/jsm/` | loads; components start after the module, as documented |
| `<script type="importmap">{raw(JSON)}</script>` | written as given |
| a JavaScript callback (`subscribe`) that assigns component state | the panels re-render on every engine change |
| `js("CandyStudio.load", text, name)` with a JSON string from a `server fn` | the engine receives the text and the name |
| `onpointerdown` with `e.currentTarget.setPointerCapture(e.pointerId)`, `onpointermove`, `onpointerup` | dragging works, with state kept in a component `state` |
| `js_new("ResizeObserver", () => ...)` in `@mount`, returning `() => ro.disconnect()` | follows the width; the ruler relabels |
| a hidden `<input type="file">` clicked from a button, `f.text()` in `onchange` | the file's text arrives (tested with CDP's `DOM.setFileInputFiles`) |
| `ask(...)` then a library call in the same handler | waits for the answer |
| `every(3000, ...)` in a component calling a `server fn` | saves the working copy to SQLite as it changes |
| `file(path, { cache: "1y" })` for generated WAV files | `audio/wav`, served in 0.4 ms once made |
| a Rust function running a 15-minute job from an MCP tool | answered after 52 s, the server serving pages meanwhile |
| `fn mcp` with a literal union parameter (`quality: "1080p" \| "720p" = "1080p"`) | an enum in the schema; the default applied |
