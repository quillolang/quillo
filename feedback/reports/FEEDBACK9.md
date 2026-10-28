# Feedback from asking whether Quillo keeps its own names apart from a program's

| | |
|---|---|
| App | None built: questions asked in one session (does Quillo now keep its reserved words and builtins apart from a program's names, and what is the risk; does `run` take a port; can a program with a service take a `.env`), answered by probing: Quillo's keywords, builtins and 14 modules as names, on the server; 46 JavaScript keywords, browser globals and runtime names as a component's state, and top-level functions, constants and props named so, in headless Chrome; then whether `quillo run` takes a port, and in what forms; then an app calling a service, with a `.env` beside them, in one process and `--split` |
| Quillo | 0.5.0 (2ad2502); Q-069 to Q-072 checked again on 9097981 (FEEDBACK8's fixes), all four still open |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check` and `quillo run` for each case; headless Chrome (`feedback/tools/drive.mjs`): each component clicked twice, `__quilloRemade` and `__quilloErrors` read, the server's and the browser's numbers compared; `feedback/run.mjs` |

What worked well: the clashes found before are gone (Q-004, Q-005, Q-012, Q-015, Q-034,
Q-035, Q-037, all fixed by f15bead), and the browser side never had them. A keyword as a name
is refused with the reason (``error: `match` is a keyword, so it cannot be a name here: pick
another name``); a builtin function hidden by a variable is warned about, where it is hidden
and where it is called (``warning: `print` hides the builtin `print` in every file of the
program, and t.qwe:2 calls the builtin``). In the browser, 46 states named like JavaScript's
keywords (`new`, `class`, `this`, `await`), the page's globals (`window`, `location`, `name`,
`history`) and the runtime's own names (`W`, `B`, `C`, `rpc`, `boot`) each counted to 2 with
no error, nothing made again, and `window.name` and `location` untouched: the compiler renames
them all. Top-level functions named `new`, `class` and `B`, constants `W`, `boot` and `window`,
and props `this` and `delete` gave the same number on the server and in the browser (65, then
66). Quillo builds no SQL from a program's names: values go in as `?`, so SQL's keywords are
only a risk in SQL the program writes.

What it cost: the one gap left is the modules. A variable named like one of them hides it,
and nothing says so (Q-069). Asked after, whether `run` takes a port: it does, as `--port 8081`
and `PORT`, but not as `--port=8081`, for any option, and a mistyped option gets no hint (Q-070).
Then whether a program with a service takes a `.env`: it does not, nor can a service have
settings of its own (Q-071), and trying one showed that `quillo check` passes a `service`
option the loader refuses (Q-072).

## Fixed, on branch feedback9-fixes

Each fix is commented in the code with its issue and the branch.

| ID | The fix |
|---|---|
| Q-069 | A module hidden by the program's top-level variable is an error where it is called and a warning where the variable is made, as Q-035's fix does for a builtin function: ``main.qwe:9: error: `json.stringify`: `json` is the program's variable from config.qwe:2, which hides the module `json` in every file: rename the variable``, and ``config.qwe:2: warning: `json` hides the module `json` in every file of the program, and main.qwe:9 calls `json.stringify` ``. A local named like a module is said to be one before its call is tried as a method (`` `math.sqrt`: `math` here is str, a variable, which hides the module `math` ``), and at run time the same, instead of "str has no method". A variable that is a map of the program's own (`let stats = { load: ... }`) is what `stats.load()` means, and is left alone (src/check.rs, src/interp.rs) |
| Q-070 | `--name=value` is `--name value` for every option that takes one (`--port=8081`, `--host=...`, `--service-url=billing=http://...`); a flag given a value is told it takes none (`` `--watch` takes no value ``); an unknown option is told the nearest (``unknown option `--prot` (did you mean `--port`?)``, `-p`: "the port is `--port`"); `-h` and `--help` after `run` print the usage (src/main.rs; the usage text and the guide) |
| Q-071 | `quillo run` reads the `.env` beside the program (or `--env-file path`) into its environment before anything reads it, so `env(...)`, `PORT` and `QUILLO_SECRET` see it; a variable the environment already has wins. A service reads the `.env` beside its own file too, in one process and under `--split` alike: its values before the app's file's, after the environment's (src/dotenv.rs; the guide). Checked with an app and a service in folders of their own, each with a `.env`, and a variable set in the shell: the same answers in one process and `--split` |
| Q-072 | `quillo check` knows the options the loader takes: ``main.qwe:7: error: service billing: no option `timout` (did you mean `timeout`?)``, and options that are not a map (src/check.rs) |

Not done from Q-071's suggestions: services finding each other through the limer pool
(`--register`) instead of `--service-url`; that changes how several servers are set up,
and is for another branch.

## Seen again

None. Q-035 is the same clash for builtin functions, fixed; this is its module side.

## New issues

### Q-069. A variable named like a module (`let json`, `let db`) hides it: `quillo check` passes, and the call fails at run time

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` on each module hidden, then `quillo run` | 2 |

Q-035's fix warns when a top-level `let` hides a builtin function. A module is not warned
about: `db`, `http`, `json`, `money`, `re`, `math`, `vec`, `stats`, `date`, `crypto`,
`channel`, `media`, `udp`, `image`. Each, hidden by a top-level `let` and used in a function:

| | `quillo check` | At run time |
|---|---|---|
| `let db = "main"`, then `db.query(...)` | ok | `str has no method `query`` |
| `let json = "x"`, then `json.stringify(...)` | ok | `str has no method `stringify`` |
| `let vec = "x"`, then `vec.argsort(...)` | ok | `str has no method `argsort`` |
| `let math = "x"`, then `math.sqrt(4)` | ok | `` `sqrt` expects a number as argument 1, got str `` |
| `let http = 1`, then `http.get(url)` | ok | `` `get` expects a map as argument 1, got int `` |

All 14 pass `quillo check`. Inside a function (`let db = "x"` then `db.query(...)`) the check
fails, with the same messages. None says that a variable hid a module, and two point
somewhere else entirely: a value's method call is a builtin called with the value first, so
`math.sqrt(4)` on a string is `sqrt("x", 4)` ("`sqrt` takes 1 argument, got 2" in the check)
and `http.get(url)` is the map function `get`.

The names are ones programs pick: `let json = request.json`, `let db = env("DB_NAME")`,
`fn f(re: str)`, `let date = row.created`, `let image = upload.file`, `let stats = load_stats()`.
A top-level one in one file hides the module in every file (as `let print` did before Q-035),
and the failure waits for a request:

```quillo
// config.qwe
let json = "settings.json"

// main.qwe
use "config.qwe"
route GET / = json.stringify({ settings: "loaded" })
```

`quillo check`: `main.qwe: ok (1 routes, 0 functions, 0 components, in 2 files)`. `GET /`:
500, and in the log `main.qwe:9: str has no method `stringify``.

Repro: [Q-069-variable-hides-a-module/](../repros/Q-069-variable-hides-a-module)
(`main.qwe` and `config.qwe`; `mode: check`).

**Suggestion:** treat a module as Q-035's fix treats a builtin function: a warning where the
variable is made, ``warning: `json` hides the module `json` in every file of the program, and
main.qwe:9 calls `json.stringify` ``, with the rename in it; and the error at the call naming
the variable (``json.stringify: `json` is your variable (config.qwe:2), not the module``)
before trying it as a method. A variable never followed by `.` (`let json = 5`, then
`json + 1`) needs neither.

### Q-070. `quillo run app.qwe --port=8081` is an unknown option, for every option, and a typo is not told what it meant

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the command line, asked whether `run` takes a port | 1 |

`run` takes an option and its value as two arguments, and nothing else:

| Typed | Said |
|---|---|
| `quillo run main.qwe --port 8081` | listens on 8081 |
| `quillo run --port 8081 main.qwe` | listens on 8081 (Q-058's fix) |
| `PORT=8081 quillo run main.qwe` | listens on 8081 |
| `quillo run main.qwe --port=8081` | ``quillo: unknown option `--port=8081` `` |
| `--host=127.0.0.1`, `--workers=4`, `--instances=2`, `--watch=true` | the same, for each |
| `quillo run main.qwe --prot 8081` | ``quillo: unknown option `--prot` `` |
| `quillo run main.qwe -p 8081` | ``quillo: unknown option `-p` `` (`-w` is the one short form) |
| `quillo run main.qwe -h` | ``quillo: unknown option `-h` `` (`quillo -h` gives the help) |

`--port=8081` is how most command lines take a value (GNU's, Node's, cargo's, Deno's), and
"unknown option `--port=8081`" reads as if there were no `--port`. A typo gets no hint,
where Quillo's own checker suggests names everywhere else ("did you mean `take`?").

Repro: [Q-070-option-equals-value-refused.qwe](../repros/Q-070-option-equals-value-refused.qwe)
(`mode: cli`, `args: run ... --port=18997`).

**Suggestion:** take `--name=value` as `--name value` for every option that takes a value;
for an unknown one, the nearest known: ``unknown option `--prot` (did you mean `--port`?)``;
and `-h` as `--help` after `run` too. `-p` could stay out: one short form is fewer to learn,
as long as the error names `--port`.

### Q-071. A `.env` beside the program is not read, and a service cannot be given settings of its own

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | an app calling a service, a `.env` beside them, run in one process and `--split` | 1 |

`env(name)` reads the process's environment, and nothing puts a file in it:

| Started as | The app's `env("API_KEY")` | The service's |
|---|---|---|
| `quillo run main.qwe` | null | null |
| `quillo run main.qwe --split` | null | null |
| `set -a; . ./.env; set +a; quillo run main.qwe` | `from-dotenv` | `from-dotenv` |
| the same, `--split` | `from-dotenv` | `from-dotenv` |

The shell's way works, quoted values too (`GREETING="hello there"`), and reaches every
service: in one process they share the app's environment, and `--split` starts each from it
([src/service.rs:1084](../../src/service.rs#L1084)). Without it, a missing variable is
`null`, found at the first request that reads it; Deno, Bun and Next.js read `.env`
themselves.

Nor can a service have settings apart from the app's. Asked whether the `service` line could
pass them (`service billing = "billing.qwe" { config: { currency: "EUR" } }`), the answer
is that it could not hold together:

- In one process a service is loaded once and shared by every program that declares it:
  the bank's gateway, `payments` and `risk` all declare `accounts`. Three `config`s, one
  service.
- A service on a server of its own (`--service-url`) is started by its own command; the
  program that calls it starts nothing there to hand settings to.
- The caller's source would hold the service's keys, which only the service needs.

And what has to be configured, by how the services run:

| Mode | To configure |
|---|---|
| One process | nothing |
| `--split` | nothing: Quillo picks the ports and makes the secret |
| Each service on its own server | `--service-url billing=http://...` (or `QUILLO_SERVICE_BILLING`) on each caller, and `QUILLO_SERVICE_SECRET` on every machine |

Repro: [Q-071-no-dotenv/](../repros/Q-071-no-dotenv) (a `.env` setting
`QUILLO_REPRO_071`; prints `key=none`).

**Suggestion:** each program reads the `.env` beside its own file, a service included
(`services/billing/.env` for `services/billing/billing.qwe`), in one process, under `--split`
and alone on its server alike; a variable already in the environment wins over the file, so
a stray `.env` cannot override production's; `--env-file path` for another. A service's own
settings are then its folder's, as its database already is. For the last row of the table:
services that register under their name in the limer pool (`--register`, already there for
live updates), and callers that find them there, with `--service-url` to override; the secret
is still set once per machine.

### Q-072. `quillo check` passes a `service` option the loader refuses: the program does not start

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `service ... { env: {...} }`, tried while answering Q-071 | 1 |

```quillo
service billing = "billing.qwe" { timout: "2s" }
```

`quillo check`: `ok (1 routes, 0 functions, 0 components)`. `quillo run`:
``service billing: no option `timout` (timeout, retries, failures, cooldown)``, and nothing
starts. The same for `{ env: { API_KEY: "x" } }`. The loader knows the four options
([src/service.rs:169](../../src/service.rs#L169)); the checker does not look. Q-049's fix
made it look at `http`'s options; this is the same gap for `service`.

Repro: [Q-072-service-option-not-checked/](../repros/Q-072-service-option-not-checked)
(`main.qwe` and `billing.qwe`; `mode: check`).

**Suggestion:** the loader's message from `quillo check`, at the option: ``no option
`timout` (did you mean `timeout`?)``.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `quillo run t.qwe` for each case, on the default port | It was one line in a loop | Port 8080 was taken: "cannot listen on 127.0.0.1:8080" stood in for the answer once. `--port` for every run since | 1 run |
| `vec.sum([1])` for the `vec` row | `math` and `stats` have their sums | `vec` has no `sum` (`` `vec` has no function `sum` ``), which looked at first like the one module that was checked; `vec.argsort` showed it hides as the others do | 1 check |
| "`let db = 5` passes with nothing said" (to the user, first) | A variable used as a value | Right, and harmless: a hidden module matters only where `db.` follows. The table above is about that | – |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `let match = 1` | ``error: `match` is a keyword, so it cannot be a name here: pick another name`` |
| `fn f(if: int) = if` | ``error: expected a parameter name, found `if`, which is a keyword: pick another name`` |
| `state route = 1` | ``error: expected a state name, found `route`, which is a keyword`` |
| `let map = 5`, then `[1, 2] \| map(...)` in one function | a warning that `map` is your variable (line 1), and an error at the call |
| `fn h(str: int) = str(str + 1)` | the same, for a parameter |
| `let print = 3` at the top | ``warning: `print` hides the builtin `print` in every file of the program, and t.qwe:2 calls the builtin`` |
| `fn push(xs, x)` | ``warning: `push` hides the builtin `push`: the program's own is used everywhere`` |
| `let request = { x: 1 }` in a route, then `request.form.name` | ``error: this map has no field `form` (it has x)``: the local is what was asked for |
| 46 component states named `new delete function this class var const typeof instanceof void await yield async arguments undefined NaN Infinity eval window document location history name self top parent event console globalThis Object Array JSON Math Promise W B C S I L N F rpc server quillo boot` | each counts to 2 after two clicks; 46 resumed, 0 remade, 0 errors; `window.name` and `location` unchanged |
| Top-level `let W`, `let boot`, `let window`, `fn new`, `fn class`, `fn B`; props `this` and `delete` | 65 on the server and in the browser, 66 after a click; resumed, nothing remade |
| `let json = 5`, then `json + 1` | works: a hidden module is no trouble until `json.` follows |
| SQL keywords | not Quillo's: it builds no SQL from a program's names, and values are bound as `?` |
| A `.env` loaded by the shell (`set -a; . ./.env; set +a`), then `quillo run --split` | the app and its service both read it: services are started from the app's environment |
