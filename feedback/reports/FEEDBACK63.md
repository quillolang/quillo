# Feedback from weighing decorators: `@mcp("path.to.about")`, and decorators made by `.qwer` plugins

| | |
|---|---|
| App | None: a design study of the user's idea of 2026-11-09: "mcp as decorator, and plugins .qwer can produce a decorator: `@mcp("path.to.about")`. Let me know what is more acceptable." |
| Quillo | `0.8.0 (94142c3, 2026-11-09)`, the release build of branch feedback56; main and feedback56 are at `92a6b25`, which changes only how a stop waits, not what this report probes |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | Probes run with `quillo check`, `quillo mcp` (stdio) and `quillo run --watch` (the HTTP door, with the token it prints), using `curl`. Read: `src/lexer.rs`, `src/parser.rs`, `src/native.rs` and `src/mcp.rs`. The 628-plugin catalog. The `quillo-editors` commits for the `fn mcp` tag. The README files of the official MCP SDKs, fetched from raw.githubusercontent.com on 2026-11-09. The scripts are at the end |

No Quillo source was changed, and neither were `feedback/INDEX.md` or `feedback/repros/`. Each repro is in its own section. Numbers start at Q-927, after FEEDBACK62.md's Q-926. Under the rule of 2026-11-05, nothing here is built until main is in sync, every test (CI too) is green and the docs are aligned.

The string in `@mcp("path.to.about")` is read here as **the tool's name, as clients see it**, apart from the function's name. MCP allows dots in tool names. Quillo writes a file path as `lib/auth`, not `path.to`, so reading the string as a module path to a function fits Quillo less. If another meaning was intended, question 2 changes; nothing else does.

One fact shapes the whole answer: **MCP is in no release yet.** `fn mcp` is on main (`e8d006f`, 2026-11-08) and not in v0.8.0 (`5562c7a`). No program outside this repo uses it. A change of spelling now costs only this repo's own tests, docs and editors. After a release it would cost users a migration.

What worked well: the question could be settled with things that already exist. The lexer already refuses `@` with a reason (`src/lexer.rs:485`). The guide says it on purpose: "There are no classes, interfaces, enums ..., decorators or exceptions" (USERGUIDE.md:497). The plugin checker says exactly why a function cannot be passed to a plugin. And the HTTP door gave the same tools a working `request`, which is what showed the stdio door's gap (Q-927).

## The answer

**`fn mcp` is the more acceptable choice. Keep it, and take two things from `@mcp("path.to.about")`: a tool name of its own (Q-928), and a checker that tells the decorator habit what to write (Q-929). Plugin decorators are acceptable only as stages, which run before a function and may answer in its place. They must never make a door, and MCP does not need them now (Q-930).**

| | A. `fn mcp` (built) | B. `@mcp(...)`, built into Quillo | C. `@` open to `.qwer` plugins |
|---|---|---|---|
| What a model writes with no guide | `@mcp.tool()`, the Python SDK's form. Today it is refused with a message that names no Quillo form (Q-929) | `@mcp.tool()` again: closer, but still wrong unless `.tool` is accepted too | B, and then `@cache`, `@login_required`, `@app.route`, `@property`, each needing an answer |
| Consistency with Quillo | The kind comes first and the tag second, as in `private fn`, `server fn`, `service mcp`, and FEEDBACK57's `route mcp` | `server` stays a word and `mcp` becomes a decorator: `@mcp server fn add_note`. Two syntaxes say what a function is | The same, plus a second way to write stages: routes already take `| stage` |
| Arguments (a name, FEEDBACK58's `shows`) | No place for them today. A string after the tag gives the name (Q-928) | They fit naturally, but Quillo has no named arguments (`f(1, c: 9)` is refused), so it would be `@mcp({ name: "...", shows: OrderCard })` or a new argument form for decorators only | The same as B |
| What `quillo check` knows | Everything, from the source | Everything, while `@mcp` is Quillo's own | Only what the plugin's file declares: names, Rust types, `//! effects:`. Not what the code does: the checker reads the file without building it (`src/native.rs:31`) |
| Editors, per new tag | Measured: the `fn mcp` tag took 28 files in `quillo-editors` (`4906e2f`), plus 5 more for Zed's and Helix's grammar pins (`8bbf72b`) | One grammar rule, written once. A new decorator needs no editor change | The same as B |
| Cost of changing now | None | Read from the code: 460 lines of `tests/feedback56.rs`, 15 lines of the guide, 13 source files that name it, 28 editor files, the website's `mcp` branch (`d51c0f1`), and the article draft (`.git/wip/medium/introducing-quillo.md:188`) | B, plus a plugin interface for stages (Part C) |
| Speed | A flag on the function (`FnDef.mcp`, `src/ast.rs:184`) | The same flag | Read, not measured: two plugin calls per call (before and after), each a JSON round trip off the event loop |

The decision between A and B, in the four impacts:

| Impact | Switching to `@mcp` before the release |
|---|---|
| **Functionality** | No change. The same flag and the same tools. Arguments gain a place to go |
| **User** | Readers who know Python, C# or Rust recognise it at once. `server` stays a word, so `@mcp server fn add_note` mixes both forms. A legal `@` invites the decorators Quillo does not have, and each one needs a message |
| **Requests** | No change |
| **Security** | No change while only Quillo makes `@mcp`. Under C, a plugin's `@` that made tools would hide a door from the checker (Part C) |

What the current spelling gives that a change would lose: one form says what a function is. The checker sees every door in the source. And it is already built, documented, and in every editor.

**My recommendation:** keep `fn mcp`. Add `fn mcp "path.to.about" about_text()` for a tool's own name (Q-928). Teach the checker the decorator habits (Q-929). Fix Q-927. The trade-off: Python readers meet one unfamiliar line, and the checker turns their first guess into the right one. What is given up is B's one real gain: one grammar rule for every future tag. That is worth taking up again if tags keep coming, as `route mcp` and `shows` already are.

## Part A: what the official MCP SDKs do

Each SDK uses what its own language already has. None invents a mechanism for MCP.

| SDK | Form | As its README writes it | A tool name apart from the function's |
|---|---|---|---|
| Python | decorator | `@mcp.tool()` over `def add(a: int, b: int) -> int` | yes: `tool(self, name: str \| None = None, title=..., description=..., annotations=...)` (`src/mcp/server/mcpserver/server.py:669`) |
| C# | attribute | `[McpServerTool, Description("Echoes the input message back")]`, in a class marked `[McpServerToolType]` (`docs/concepts/tools/tools.md`); factory methods also | not checked |
| Rust | attribute macro | `#[tool(description = "Add two numbers")]` in a `#[tool_router]` impl | not shown in the README |
| TypeScript | function call | `server.registerTool('greet', { description, inputSchema }, handler)` | always: the name is the first argument |
| Go | function call | `mcp.AddTool(server, &mcp.Tool{Name: "greet", Description: "say hi"}, SayHi)` | always |
| Kotlin | function call | `server.addTool(name = "echo", description = ..., ) { request -> ... }` | always |
| Java | function call in the SDK; annotations through Spring AI's MCP Annotations, which the README links | not read | not read |

Three SDKs use decorators or attributes, and three use calls. Python's form has the most examples in what models have read, which is why a model's first guess is `@mcp.tool()` (Q-929). In the SDKs that use calls, a tool's name is always its own and never the function's (Q-928). Quillo's own way to say what a function is, is a word in the head.

## Part B: `@mcp` built into Quillo, worked through

```quillo
/// The site's About text.
@mcp("path.to.about")
fn about_text() -> str with db = db.one("select body from pages where slug = 'about'").body

@mcp
server fn add_note(text: str) -> list ! str with db { ... }

@mcp
service /billing = "billing.qwe"

@mcp
route GET /orders/:id(int) with db = <html><body><OrderPage id={id}/></body></html>   // FEEDBACK57's route mcp

@mcp({ shows: OrderCard })                                                            // FEEDBACK58's shows
fn order(id: int) -> Order with db = load_order(id)
```

What it gains:

- **A place for arguments.** `fn mcp(` already means a function named `mcp`, so the tag cannot take arguments in parentheses. A decorator line can.
- **A short head.** `private server fn mcp name(...) -> T ! E with db, http for ["shop"] shows OrderCard` is long. A line above it takes some of that.
- **One grammar rule.** Each tag so far has cost every editor a change: 28 files for `fn mcp` alone. `route mcp` and `shows` are next.

What it costs:

- **`@` means two things.** In markup it starts an event (`@click`, `@submit`). Vue with TypeScript decorators lives with that, so this is the smallest cost.
- **Two syntaxes for what a function is.** `private`, `server` and `mcp` are all doors or limits on doors. Making one of them a decorator leaves the reader asking why the others are words. Moving all of them is a rewrite of every head in the language.
- **It reverses a stated design.** The guide (line 497), the book (`14-language-tour.md:133`), the lexer's message, and FEEDBACK29's table row (`@cache` gets "no decorators") all say Quillo has none.
- **A legal `@` invites everything else.** Once `@mcp` parses, `@cache` stops being "no decorators" and becomes "no decorator named cache". That reads as a gap, and plugin decorators (Part C) are what it asks for.
- **Named arguments.** Quillo refuses `f(1, c: 9)` (FEEDBACK29) and passes a map instead. So `@mcp(shows: OrderCard)` is either `@mcp({ shows: OrderCard })` or a second argument syntax that works only in decorators.

## Part C: decorators made by `.qwer` plugins

**What a plugin is today** (`src/native.rs:16-31`). A `.qwer` file is Rust. Its `pub fn`s take and return values: numbers, text, booleans, lists, maps, `Option`, `Result` and `serde_json::Value`. Calls go through JSON and run off the event loop, without the lock Quillo code holds. The checker reads the signatures without building the file. A function cannot cross. Measured:

```rust
pub fn wrap(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }
```

```
./stage.qwer:4: warning: `wrap` is not exported: `fn(i64) -` cannot cross between Rust and Quillo (numbers, bool, String, Vec, HashMap<String, _>, Option, Result and serde_json::Value can)
```

So a plugin cannot be a Python decorator, which takes a function and gives back one that calls it. Allowing that would mean Rust calling back into the interpreter from off the event loop, which the design avoids on purpose.

**What a plugin could be: a stage.** Quillo calls the plugin before the body, with the call's facts as a map: the function's name, its arguments, the caller. The plugin either lets the call go on or answers in its place. It could also be called after, with the result. That is route middleware for functions. The catalog already has what such stages would call: `rate_limit`, `memory_cache`, `cache_disk`, `redis`, `http_cache`, `prometheus`, `otlp`, `sentry`, `hash_chain` (an audit log). Today a program wraps them as route-group middleware. `rate_limit.qwe` writes `fn limited(req) { ... rate_limit(...)? ... }` and then `routes /api | limited { ... }`.

**What a plugin must never do: make a door.** Three things depend on every tool being known from the source:

- the MCP security review (`94142c3`), which refuses a question in a tool's call wherever it is reached;
- the checker's warnings about questions after a change (`064911f`), and about idempotent runs reaching a function that changes data (`92a6b25`);
- the "both authors agree" rule for `service mcp` (FEEDBACK56, Part F).

A plugin is Rust that is built after the check. If `@something` from a plugin could make a tool, the checker would have to accept a door it never saw. So a plugin stage may let a call through, answer it, or (after the body) change the result, and nothing else. Its effects are its file's `//! effects:` line, and they must fall within the function's `with`, as the function's own calls must.

**MCP itself as a plugin's decorator?** No. `mcp.qwer` in the catalog is an MCP client, plus the JSON-RPC answers a route can give. The door needs things no plugin sees: the function's types and `///` comment, re-running the function for `confirm` and `about`, sessions and tokens, and the read-only transaction. This agrees with the user's decision of 2026-11-09: no separate MCP process.

**What tools need from stages is the door's already:** who is calling (a session or a token), limits (60 calls a minute, `--max-tool-calls`), hidden internals, and the read-only transaction. A rule for one tool, such as a role, is one line in its body. So stages on functions are a question for the language as a whole, not something MCP is waiting on (Q-930, question 3).

## Seen again

None. Q-929 extends Q-520's fix, which taught the checker other languages' keywords, to `@`.

## New issues

### Q-927. A `server fn mcp` that reads `request` passes `quillo check`, works over HTTP, and fails every call over `quillo mcp`

| Kind | Detected by | Attempts |
|---|---|---|
| silent (to the checker) | `quillo mcp` and the HTTP door on the same program | 1 |

```quillo
/// Who the client says it is.
server fn mcp who() -> str = str(request.mcp)
fn guard(r) = r
/// The guide's stage.
server fn mcp staged() -> str = request | guard | (_) => "ok"
```

`quillo check` says ok. Over HTTP, `who` answers `{ client: "probe", version: "1" }` and `staged` answers `ok`. Over stdio, both answer `isError` with `internal error`, and stderr says `unknown name `request``. Both forms come from the guide. It says `request.mcp` "is read where `request` is, in a `server fn mcp`" (USERGUIDE.md:2905). It also writes a server fn's middleware as `request | auth.required | (_) => ...` (USERGUIDE.md:2729). The stdio section (USERGUIDE.md:2835) does not mention that there is no `request` there. A tool written and tried with `quillo run` breaks as soon as a user adds it to their editor with `quillo mcp`. The developer sees it only in the client's stderr.

| Impact | |
|---|---|
| **Functionality** | Every call to such a tool fails over stdio, and works over HTTP |
| **User** | The AI client gets `internal error`. The developer has no warning from `quillo check`, and the real message is in the client's log |
| **Requests** | No change over HTTP. Over stdio, each call is answered with `isError` |
| **Security** | No change: it fails closed |

**Suggestion:** under `quillo mcp`, make `request` a map with `mcp` set from the client's `_meta`, and empty `headers`, `cookies` and `query`. Then `request.mcp` works, and a stage that looks for a header finds none and answers as the program chose. If a stage refuses without an `Authorization` header, it refuses on stdio: the developer sees that answer at once, not `internal error`. Say in the guide's stdio section what `request` holds there. Test: both tools above over stdio, and the stage's refusal reaching the client as its own text.

### Q-928. A tool's name is its function's name: it cannot have dots, it shares names with builtins, and renaming the function renames the tool for every client

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` on 55 common tool names; `fn mcp path.to.about()` | 1 |

```quillo
fn mcp path.to.about() -> str = "hi"   // error: expected `(`, found `.`
/// t
fn mcp count() -> str = "x"            // warning: `count` hides the builtin `count`: ... pick another name
```

Of 55 common tool names, 19 hide a builtin, so the program loses that builtin everywhere: `about`, `ask`, `confirm`, `get`, `set`, `find`, `sleep`, `now`, `date`, `print`, `sort`, `filter`, `map`, `count`, `sum`, `max`, `min`, `round`, `notify`. Following the warning renames the tool. Dots appear only through a service (`billing.invoice`), one level deep. Any rename of a function in code is a rename for clients. MCP clients key their saved permissions by tool name: Claude Code names a server's tools `mcp__<server>__<tool>` and keeps its allow rules under that name. So a refactor drops every user's "always allow" for that tool, and their clients see the new name only when the 5-minute `ttlMs` runs out (`6e19ace`).

| Impact | |
|---|---|
| **Functionality** | No change to calls. A name the developer wants may be impossible, or may cost them a builtin |
| **User** | The developer renames the tool to keep a builtin. The client's user is asked again for permission after a refactor |
| **Requests** | After a rename, clients call the old name and get `-32602` until they list the tools again |
| **Security** | No change: a name is not a permission on the server's side |

**Suggestion:** a string after the tag is the tool's name: `fn mcp "path.to.about" about_text() -> str`. This is `@mcp("path.to.about")` in the head, with the kind still first. The parser can tell it apart: after `fn mcp`, a `(` means a function named `mcp`, a name means a tool, and a string means a tool with its own name. `quillo check` holds the string to MCP's rules (1 to 128 characters of `A-Z a-z 0-9 _ - .`), and refuses two tools with the same name in a program, counting `service mcp` prefixes. Under `service mcp /billing`, billing's `"invoices.create"` is `billing.invoices.create`. The same string after `route mcp` for FEEDBACK57, if wanted. Test: the parse of all three forms; each refusal; a name equal to another function's; a service's prefixed name; `tools/list` and `tools/call` by the string.

### Q-929. `@mcp.tool()`, `@tool`, `@app.route("/")` and `@cache` all get the same message, which names no Quillo form

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

```
@mcp.tool()
fn add(a: int, b: int) -> int = a + b
// py.qwe:1: error: unexpected character `@`: there are no decorators, and in markup it starts an event, as `@click`
```

Q-520's fix taught the checker other languages' keywords, each with how Quillo writes it. `@` gets one sentence for every habit. The most common habit is the Python SDK's own form (python-sdk's README, line 53), and Quillo has an exact answer for it.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A developer or model coming from Python has to find `fn mcp` in the guide instead of being told it |
| **Requests** | No change |
| **Security** | No change |

**Suggestion:** read the name after `@` and answer with the Quillo form:

| Written | Told |
|---|---|
| `@mcp.tool`, `@mcp`, `@tool`, `@server.tool` | `` a tool is `fn mcp name(...)`, with a `///` comment for its description `` |
| `@app.route`, `@app.get`, `@router.get`, `@get` | `` a route is `route GET /path = ...` `` |
| `@login_required`, `@auth`, `@requires_auth` | `` a check before routes is middleware: `routes / \| signed_in { ... }` `` |
| `@cache`, `@lru_cache`, `@functools.cache`, `@memoize` | `` no decorators: keep the value in a global, or use `memory_cache.qwer` `` |
| `@property`, `@staticmethod`, `@classmethod`, `@dataclass` | `` there are no classes: `type` and functions `` |
| anything else | today's message |

Test: one per row, and that `@click` in markup is not affected.

### Q-930. Functions have no stages of their own: routes take middleware, a `server fn` takes one only inside its body, and a plain `fn mcp` cannot take one at all

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check` on a stage in an `fn mcp`; the guide | 1 |

```quillo
routes /api | limited { route GET /orders = ... }                // a stage for many routes
server fn delete_all() = request | auth.required | (_) => { ... } // one function, in its body (USERGUIDE.md:2729)
fn mcp plain() -> str = request | limited | (_) => "ok"           // error: `request` only exists in routes and `server fn`s
```

This is the gap that "plugins can produce a decorator" points at. A check that many functions share has to be written in each function's body. In a `server fn`, the real body then sits inside a lambda. A plain `fn mcp` has no `request` to give a stage, and with Q-927, a `server fn mcp` has none over stdio either.

| Impact | |
|---|---|
| **Functionality** | No change. Each function calls the check itself |
| **User** | The same line is repeated in every function, and is easy to leave out of one |
| **Requests** | No change |
| **Security** | A check left out of one function is a hole. For tools, the door already covers the common checks: who is calling, limits, hidden errors |

**Suggestion (question 3):** not now. If functions get stages, give them the rules of Part C. A stage is given the call's facts (name, arguments, caller) rather than an HTTP request, so it works on both doors and in plain functions. It may let the call go on, answer in its place, or change the result. It never makes or opens a door. Its effects fall within the function's `with`. The checker knows it from the source, or from the plugin's header without building it. The spelling should be decided then: an `@stage` line above the function, or a group as routes have (`fns | limited { ... }`). Under that rule, `@` would mean one thing, "runs around this function". `fn mcp` would stay a word, because a door is not a stage.

## Proposed order of work

Each item has its tests and edge tests, in `tests/feedback63.rs`. Coverage stays at 99%, and the guide and the book change in the same slice.

1. **Q-927**, the stdio `request`. Small, and a bug in what is about to be released.
2. **Q-929**, the decorator habits. Small, and messages only.
3. **Q-928**, a tool's own name. The parser, the checker, `tools/list` and `tools/call`, `service mcp` prefixes, `src/highlight.rs`, the language server, tree-sitter and every editor (one more form of the tag), the guide, the book, the website's `mcp` branch.
4. **Q-930** only after question 3, as a study of its own.

All of it before MCP is released, so that the release has the name form from the start.

## Open questions for the user

1. **`fn mcp` or `@mcp`?** Keep `fn mcp` (recommended: one form says what a function is, the checker sees every door, and it is built), or change to `@mcp` before the release, while only this repo uses it? If you choose `@mcp`, I would write the slice that moves every use (tests, guide, book, editors, website branch, article) as one change, with `fn mcp` refused, and a message that names `@mcp`.
2. **A tool's own name:** `fn mcp "path.to.about" about_text()` (recommended), or did `"path.to.about"` mean something other than the tool's name? If no own name is wanted, the tool keeps its function's name, and Q-928's builtin warnings stay as they are.
3. **Stages on functions, which is what plugin decorators would be:** leave them for now (recommended: the door already covers what tools need), or study them next with Part C's rules? If you say leave them, `@cache` and the others get Q-929's messages, and nothing else changes.

## Smaller notes

- The checker's message for setting a global map's field in a plain function, `calls.n += 1`, ends "in a component", though the code was in no component: ``cannot set `n`: only the page's objects (an element, an event) have fields that are set, in a component``.
- If `@` is ever allowed above declarations, the tree-sitter grammar must keep `@click` in markup apart from it, as the lexer does now.

## Where the AI struggled

| What was tried | What happened | Whose mistake |
|---|---|---|
| A zsh `for w in $names` loop over 55 tool names | zsh does not split a variable into words, so the probe ran once and reported every name as colliding. Rerun as a bash script | the AI's |
| `echo =====` between commands | zsh expands `=word`, so the command failed with "===== not found" | the AI's |
| Reading the SDK READMEs through the web-fetch agent | GitHub returned 504 for five of seven pages, twice. `raw.githubusercontent.com` with `curl` worked for all | the tool's |
| A first stage probe counting calls with `calls.n += 1` on a global map | refused (smaller notes): a JavaScript habit | the AI's |
| A stage in a plain `fn mcp` | `request` only exists in routes and `server fn`s (Q-930), so the probe moved to `server fn mcp`, and that is how Q-927 was found | neither: by design |

## Edge cases checked that behave well

- `fn mcp(x) = x + 1` is still a function named `mcp` next to tools in the same file.
- `@click` and `@submit` in a component's markup check fine in the same file as `fn mcp(x)` and an `fn mcp` tool (`click.qwe`). They are the article's own forms (`.git/wip/medium/introducing-quillo.md:27`).
- The HTTP door gives a `server fn mcp` its `request`, with `request.mcp` set from the client's `_meta`.
- A `.qwer` function whose parameter is a Rust function is not exported, with a warning that names what can cross. The rest of the file still checks, and its other function can be called.
- A tool's description comes from its `///` comment, its input schema has `additionalProperties: false`, and a tool that only reads gets `readOnlyHint: true` and `openWorldHint: false` (`tools/list` on `tag.qwe`).

## Scripts

Run from a scratch folder with `Q=./target/release/quillo` from the repo.

The decorator, the tag and a dotted name:

```sh
printf '@mcp("path.to.about")\nfn about() -> str = "hi"\n' > deco.qwe
printf '/// Says hi.\nfn mcp about() -> str = "hi"\n/// Totals.\nserver fn mcp total(a: int, b: int) -> int = a + b\nfn mcp(x) = x + 1\n' > tag.qwe
printf 'fn mcp path.to.about() -> str = "hi"\n' > dotted.qwe
for f in deco tag dotted; do $Q check $f.qwe; done
M='"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}'
printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{%s}}\n' "$M" | $Q mcp tag.qwe
```

The 55 names, in bash:

```bash
for w in about ask confirm search fetch read write list get set find open delete update create send upload download \
  translate summarize query run exec sleep now time date log print format parse sort filter map count sum max min round \
  hash encode decode notify email share lookup status health version weather echo add greet calculate convert; do
  printf '/// t\nfn mcp %s() -> str = "x"\n' $w > n.qwe; $Q check n.qwe 2>&1 | grep -q "hides the builtin" && echo $w
done
```

The habits: `@mcp.tool()`, `@tool` and `@app.route("/")` above a function, each through `quillo check`.

Q-927, the same `who.qwe` (above, in its section) through both doors:

```sh
M='"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"probe","version":"1"}}'
for t in who staged; do
  printf '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"%s","arguments":{},%s}}\n' $t "$M" | $Q mcp who.qwe
done                                                   # both: isError, "internal error"; stderr: unknown name `request`
$Q run who.qwe --watch --port 47963 > run.log 2>&1 &   # prints a token for the door
TOK=$(grep -oE "Bearer [A-Za-z0-9._~+/=-]+" run.log | head -1 | cut -d' ' -f2)
for t in who staged; do
  curl -s http://127.0.0.1:47963/_quillo/mcp -H 'content-type: application/json' -H "authorization: Bearer $TOK" \
    -H 'MCP-Protocol-Version: 2026-07-28' -H 'Mcp-Method: tools/call' -H "Mcp-Name: $t" \
    -d "$(printf '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"%s","arguments":{},%s}}' $t "$M")"
done                                                   # who: { client: "probe", version: "1" }; staged: ok
kill %1
```

The plugin with a function parameter (`stage.qwer`, with `use "stage.qwer"` in `m.qwe`):

```rust
//! A stage.
//! version: 0.1.0

pub fn wrap(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }
pub fn before(call: serde_json::Value) -> Option<serde_json::Value> { None }
```

The SDK READMEs: `https://raw.githubusercontent.com/modelcontextprotocol/<repo>/main/README.md` for `python-sdk`, `typescript-sdk`, `csharp-sdk`, `java-sdk`, `rust-sdk`, `go-sdk` and `kotlin-sdk`; C#'s attribute in `csharp-sdk/main/docs/concepts/tools/tools.md`; Python's `name` option in `python-sdk/main/src/mcp/server/mcpserver/server.py`. The editors' cost: `git -C ~/dev/quillo-editors show --stat 4906e2f 8bbf72b`.
