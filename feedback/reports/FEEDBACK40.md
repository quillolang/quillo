# Feedback from asking whether `quillo run app.qwe --split` parses the whole app

| | |
|---|---|
| App | None. The question: when `--split` starts each service as a process of its own, does Quillo still parse the entire app? Who reads which file, how often, and what it costs |
| Quillo | 0.6.0 (5a919f0, 2026-11-02): the release build of `feedback27-35-fixes` at its last commit, built for this report (the tree clean but for `mobile/quillo-mobile/Cargo.lock`) |
| Model | Claude (Opus 5.5) |
| Tested with | Small apps in a scratch folder: an app with one service (`billing`), an app with a service of a service (`billing` declares `tax`), a 36,001-line `billing.qwe` (4,000 routes, 4,001 functions) for timing, `--split --watch` with a file edited while it runs, and the app run with `--service-url` and no `billing.qwe` beside it. Read: [src/service.rs](../../src/service.rs) (`statements`, `offered`, `declared`, `declare`, `split`), [src/check.rs](../../src/check.rs) (`expand`), [src/main.rs](../../src/main.rs) (`run_program`, `load`), USERGUIDE.md (Services) |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; the repros are in each section and the scripts are at the end. Numbers start at Q-471, after FEEDBACK39.md's Q-470.

What worked well: the split itself is clean. The app never runs a service's top level: a `print` at the top of `billing.qwe` shows only once, marked `[billing]`, from billing's own process. The app opens no service database and runs no service migration. A service of a service is found and started too, and the services' processes stop when the app stops. Parsing is cheap: checking a 36,001-line service takes 30 ms.

## The answer

Yes. The process you start parses every file of the program: its own files, each service's files, and the files of the services they declare. With the static check on (the default), it also checks each service in full. It does not run them. It keeps only each service's `server fn`s, to know what `billing.add(...)` takes and to send each call's contract.

Each service's process parses and checks only its own files and those of the services it declares. It does not read the app.

### Who reads `billing.qwe`, and when

For `main.qwe` with `service billing = "billing.qwe"`, run as `quillo run main.qwe --split`:

| # | Process | Where | What | With `--no-check` |
|---|---|---|---|---|
| 1 | app | `service::declared` ([service.rs:457](../../src/service.rs)), from `split` before anything starts | parses it, and its `use`d files, to find the services to start | yes |
| 2 | app | the check's `expand` ([check.rs:1659](../../src/check.rs)), `offered` | parses it for its `server fn` signatures | no |
| 3 | app | the check's `expand` ([check.rs:1678](../../src/check.rs)), `check_file_opts` | parses and checks it as a program of its own | no |
| 4 | app | the check's class words ([check.rs:5128](../../src/check.rs)) | reads its text (not parsed), for the classes it may write | no |
| 5 | app | `service::declare` ([service.rs:1163](../../src/service.rs)), as the app loads | parses it for its `server fn`s; since it has an address, it is not loaded | yes |
| 6 | billing | its check | parses and checks it | no |
| 7 | billing | its load | parses and runs it | yes |

So one service file is parsed six times and checked twice at start (three parses with `--no-check`). With `--describe`, the app parses it once more for the document's routes. A service one level down (`tax`, declared by `billing`) is checked three times: by `tax`, by `billing` and by the app. A warning in `tax.qwe` prints three times:

```
[tax] tax.qwe:2: warning: unused variable `unused` (name it `_unused` if that is intended)
[billing] ./tax.qwe:2: warning: unused variable `unused` (name it `_unused` if that is intended)
./tax.qwe:2: warning: unused variable `unused` (name it `_unused` if that is intended)
```

### What it costs

The release build, with the 36,001-line `billing.qwe`, timed from start until `GET /` on the app answers 200 (each run three times):

| Run | Time to the first answer |
|---|---|
| `quillo check billing.qwe` (the service alone) | 30 ms |
| `quillo check main.qwe` (one route, plus billing's check) | 50 ms |
| `quillo run main.qwe` (one process) | 80 ms |
| `quillo run main.qwe --no-check` | 40 ms |
| `quillo run main.qwe --split` | 200 to 275 ms |
| `quillo run main.qwe --split --no-check` | 105 ms (720 ms the first time, from a cold disk cache) |
| `--split` with a one-line `billing.qwe` | 65 to 75 ms |

The repeated parsing costs tens of milliseconds even at 36,000 lines. Speed is not why it matters. What matters is what the app needs to have, and what it stops on. Q-471 and Q-472 are those cases. Q-473 is the repeated work and the repeated messages.

## New issues

### Q-471. The app needs each service's source, and stops on any error in it, even when the service runs elsewhere

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, docs | run-time error | 1 |

`declare` reads the service's file to learn its `server fn`s before it looks at where the service runs ([service.rs:1163](../../src/service.rs), then 1171). The app therefore cannot start on a machine that does not hold the service's code, even with the service's address given and the check off:

```sh
# app/main.qwe:   service billing = "billing.qwe"
#                 route GET / = str(billing.add(1, 2))
# svc/billing.qwe: server fn add(a: int, b: int) -> int = a + b
cd svc && quillo run billing.qwe --port 47114 --as-service &
cd app && quillo run main.qwe --port 47115 --no-check --service-url billing=http://127.0.0.1:47114
```

```
quillo: main.qwe:1: cannot read ./billing.qwe: No such file or directory (os error 2)
```

Under `--split`, a syntax error anywhere in a service stops everything before any process starts, even in a part the app never calls (a route of billing's own), and even two levels down:

```quillo
// billing.qwe
server fn add(a: int, b: int) -> int = a + b
route GET /report = {
```

```
quillo: billing.qwe: line 4: unexpected end of input: missing `}`
```

```
quillo: tax.qwe: line 3: expected a parameter name, found `=`
```

The second message comes from an app whose own file and whose service `billing` are fine. Nothing says that `tax.qwe` is `billing`'s service.

A service can be built and deployed apart, but not on its own: the app depends on the service's whole source, not on its interface. Each deploy of the app needs the service's code as it is now, its bodies included. The guide's Services section says a service "runs elsewhere" with `--service-url` and does not say that its file must still be beside the app.

**Suggestion:** Learn the signatures from what the service publishes, not only from its file. Three ways, smallest first:

1. Parse a service's file for its top-level `server fn` headers only, skipping bodies and everything else. An error in a route or a body then no longer stops the app. The file is still needed.
2. An interface file. `quillo describe billing.qwe --interface > billing.qwei` writes the `server fn` headers and their contracts; `service billing = "billing.qwei"` takes them as they are. The app's repository can then hold the interface without the code.
3. With an address and no file, ask the service at start for its signed contract list (the contracts the calls already carry, [service.rs:1167](../../src/service.rs)), and say so if it cannot be reached.

Until then, the guide should say that the file must be present.

### Q-472. Under `--split --watch`, a change to a service does not reach the app

| Kind | Detected by | Attempts |
|---|---|---|
| silent | run-time error | 1 |

The app learns a service's `server fn`s once, when it loads. Under `--split`, the service's files are not among the files the app watches: in one process they are, through `svc.files()` ([service.rs:1204](../../src/service.rs)), but that path is only taken when the service is loaded locally. Billing's own process reloads, the app does not, and a function added to billing stays unknown until the app's own file changes:

```quillo
// main.qwe
service billing = "billing.qwe"
route GET / = str(billing.add(1, 2))
route GET /g = billing.greet("Ana")
```

```quillo
// billing.qwe, at start
server fn add(a: int, b: int) -> int = a + b
```

```sh
quillo run main.qwe --split --watch --no-check
# then add to billing.qwe:  server fn greet(name: str) = "hello " + name
```

| Step | `GET /g` |
|---|---|
| at start | `internal error: main.qwe:3: map has no method \`greet\`` |
| `greet` added; log says `[billing] quillo: reloaded billing.qwe` | `internal error: main.qwe:3: map has no method \`greet\`` |
| `main.qwe` touched; log says `quillo: reloaded main.qwe` | `hello Ana` |
| the same without `--split` | `hello Ana` as soon as `greet` is added |

With the check on (no `--no-check`) it is the same. `billing.add` changed to take a third parameter, `c: int = 0`, and `greet` added, in one edit; billing reloads, and the app keeps the errors it had at start, its pages answering "The program did not load":

```
main.qwe:3: error: `billing.add` takes 2 arguments, got 3
main.qwe:4: error: service `billing` has no server fn `greet` (it has add)
[billing] quillo: reloaded billing.qwe (0 routes)
```

**Suggestion:** Under `--split`, the app watches each service's files (and their `use`d files) as it does in one process, and re-runs `declare` when they change. It reloads only when the set of `server fn`s or their parameters changed, since a change to a body is the service's own business.

### Q-473. Each level checks every service below it again, and says each warning once per level

| Kind | Detected by | Attempts |
|---|---|---|
| perf, loud | reading the runtime, the log | 1 |

As the table above shows, the app checks every service in full, and each service checks its own services in full. A service N levels down is checked N + 1 times, and each of its warnings is printed N + 1 times, under two different paths (`tax.qwe` and `./tax.qwe`), so a tool that reads the log counts them as different warnings. At start each service's file is also parsed three times by the app alone (`declared`, `offered` in the check, `offered` in `declare`) and once more by the check.

The order makes it worse. `split` starts the services before the app checks anything ([main.rs:1013](../../src/main.rs), then the check at [main.rs:723](../../src/main.rs)). An error in a service is found first by the service's process, which exits, and the app then waits for it (Q-474). The app's own check of the same file, which would have said the same thing before anything started, never runs.

**Suggestion:** Under `--split`, check the whole tree once in the app before starting any service. Print it once, then pass the services a flag that skips their own check (an internal one, like `--as-service`, so a service started by hand still checks itself). In one process, keep a file's parse by path and modification time, so `declared`, `offered` and the check share it.

### Q-474. A service that fails at start makes `--split` wait 20 seconds before saying so

| Kind | Detected by | Attempts |
|---|---|---|
| loud, slow | run-time error | 1 |

`split` waits for each service's port to answer, for up to 20 s ([service.rs:1670](../../src/service.rs)), and does not look at whether the process has already ended. A service with a check error exits within milliseconds, but the app still waits the full time:

```quillo
// billing.qwe
server fn add(a: int, b: int) -> int = a + b
server fn report() = no_such_function(1)
```

```
$ time quillo run main.qwe --port 47107 --split
quillo: service billing (billing.qwe) on http://127.0.0.1:51104
[billing] billing.qwe:2: error: unknown name `no_such_function`
quillo: service billing did not start (see its lines above)
real 20.00
```

The guide documents the 20 s ("a service that is not up within 20 s stops the app with exit 1"), but it does not say that a service that has already exited is waited for too.

**Suggestion:** In the wait, call `try_wait` on each child. If one has ended, stop at once with `service billing stopped at start (exit status 1; see its lines above)`. With Q-473's order, the error would be found before anything starts.

## Smaller notes

- The app keeps each service's `server fn` definitions whole, bodies included (`offered` returns `Arc<FnDef>`), though `namespace` and the contracts need only the parameters, the return type and the name. Each body stays in the app's memory, unused, for as long as it runs.
- `--scale billing=3` starts three processes. Each parses and checks `billing.qwe` (and its services) for itself, so the counts above apply to each copy.

## Where the AI struggled

- My first timing loop passed `"--split --no-check"` as one argument. zsh does not split an unquoted variable, so `quillo` refused the option, never answered, and the loop reported a 60 s timeout as if `--split --no-check` hung. Passed as two arguments, it starts in about 100 ms. The refusal was in the log the script discarded.
- Reading the code, I took the parent's check to be the place where a service's check error is reported. Running it showed the service's own process reports it first, and the app never gets to its own check (Q-473, Q-474).

## Scripts

`run.sh DIR PORT SECONDS [flags]`: runs `quillo run main.qwe` in DIR, requests `/`, and stops only the PID it started:

```bash
Q=~/dev/quillo-app1/target/release/quillo
dir=$1; port=$2; secs=$3; shift 3
cd $dir
$Q run main.qwe --port $port "$@" > out.log 2>&1 &
pid=$!
for i in $(seq 1 $((secs*10))); do curl -s -o /dev/null localhost:$port/ && break; sleep 0.1; done
echo "--- GET / :"; curl -s localhost:$port/; echo
kill $pid 2>/dev/null; wait $pid 2>/dev/null
echo "--- output:"; cat out.log
```

`ready.py DIR PORT flags...`: milliseconds from start until `GET /` answers:

```python
import subprocess, sys, time, urllib.request
d, port, flags = sys.argv[1], sys.argv[2], sys.argv[3:]
t = time.time()
p = subprocess.Popen([QUILLO, "run", "main.qwe", "--port", port] + flags, cwd=d,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
while True:
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1).read(); break
    except Exception:
        if time.time() - t > 60: print("timeout"); break
        time.sleep(0.01)
print(f"{' '.join(flags)}: {int((time.time()-t)*1000)} ms")
p.terminate(); p.wait()
```

The 36,001-line service was 4,000 copies of a ten-line function and a route that calls it, after `server fn add`.
