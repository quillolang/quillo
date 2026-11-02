# Feedback from testing the `quillo` binary itself, and the edge cases every web server meets

| | |
|---|---|
| App | None. Part 1: the command line, the files a program is made of, `.env`, signals, ports, the output streams, and the commands besides `run` (`check`, `docs`, `fix`, `build`, `repl`, `lsp`, `dap`, `version`). Part 2: the edge cases from OWASP's testing guide and HTTP/1.1's framing rules (request smuggling, paths, XSS, cross-site requests, redirects, sign-in timing, caching and framing, files, JSON, query values, ranges), tried against a small server |
| Quillo | 0.6.0 (6ec94b4, `origin/main`, 2026-11-01), built from `git archive 6ec94b4` in a scratch folder. The checkout holds another session's uncommitted `feedback29-fixes`, which changes `src/main.rs` (scripts as tools, Q-350 and Q-357): what is reported here on scripts' arguments and exit codes may already be moving |
| Model | Claude (Opus 5.5) |
| Tested with | the shell (zsh on macOS 26), curl, `kill`, `lsof`, `pgrep`, Python for raw requests and for the language server's messages, headless Chrome (`feedback/tools/`) for redirects; each finding run on the build above |

What worked well: the parts the earlier reports fixed hold. `--port=8081` works. A misspelt option is told the nearest (`--wacth`, `--strict-nul`), and a flag given a value is refused. `--host ::1` and `--host ::` are bracketed as they should be, and `--port 0` names the port it got. `check` refuses a second program. Stopping a server that is serving is as the guide says, in every case tried: a slow request is answered before the exit, a stream ends, a request that computes for ever is named and dropped after the drain, and a second signal exits 130. The language server answered every request through eleven malformed or out-of-range messages. `fix` run twice changes nothing more.

Fix Q-378 first: anyone who can reach the server can write into its log, and so into the terminal of whoever reads it. Then Q-390, Q-391 and Q-379: the sign-in check two apps wrote is an open redirect, signed-in pages say nothing to caches or frames (and a program cannot add a header to all of them), and a server whose output pipe closes answers 500 on every page that prints. Q-380 and Q-392 lose a setting or a file's new content without a word.

Part 2 also found much that holds, which is the larger part of its table at the end: Quillo's own HTTP parser refused all 26 malformed framing cases tried, every request-smuggling shape among them, and its escaping, URL and cross-site guards held against every variant tried.

No Quillo source was changed, and neither `feedback/repros/` nor `feedback/INDEX.md` was touched, as another session is working in the checkout. The repros are inline, each run on the build above, on macOS. Numbers go on from FEEDBACK30's Q-377.

## Seen again

None was hit as it stands. Two are close:

| ID | Where it bit | Attempts |
|---|---|---|
| Q-357 | A script as a command: Q-387 (`#!`), Q-379 (a closed pipe) and Q-388 (the REPL from a pipe) are the parts its fix on `feedback29-fixes` does not cover in the guide as it stands there | |
| Q-059 | Its fix made `quillo run` with no program find `main.qwe`. Q-389 (`quillo tool` printing the usage) and Q-381's `quillo run -- a.qwe` are the same question asked again | |
| Q-022 | `fn echo() = { method: request.method, ... }` for the test server: `` `request` only exists in routes and `server fn`s ``, as documented; passed it as an argument | 1 check |
| Q-327 | Its plugin case has a JSON twin: `{"n": 18446744073709551616}` in a body is `1.8446744073709552e+19` in `request.json`, a float with no word; and `-0` becomes `-0.0` | |

## New issues, part 1: the binary

### Q-378. The log writes what a visitor sent as it came: control bytes from a bad request line, and line breaks inside an error's text

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | raw requests, `cat -v` on the log | 1 |

A request whose line or headers are malformed is answered 400 and logged with its line as it was received. Control bytes are kept, and terminal escape sequences reach whoever reads the log on a terminal (`tail -f`, `journalctl -f`), as clearing the screen or changing colours:

```sh
quillo run main.qwe --port 18475 --log json
python3 -c 'import socket; s=socket.create_connection(("127.0.0.1",18475)); s.sendall(b"GE\x1b[2JT / HTTP/1.1\r\nHost: x\r\n\r\n"); s.recv(99)'
# the log, through cat -v:
# GE^[[2JT / HTTP/1.1 -> 400 (bad request)
```

That line is text under `--log json` too, so a collector that reads one JSON object per line meets one it cannot parse. Quillo's own notices (`listening on`, `stopping once...`, warnings) are text there too. The guide does not say whether those count as the log.

An error's text is written as it is, line breaks included. A program's error that holds what a visitor sent, as `` err(`no user ${name}`) `` does, lets the visitor add lines of their own to the text log, made to look like Quillo's:

```quillo
route GET /crash/:m = err(`bad "${m}"`)
```

```sh
curl '127.0.0.1:18474/crash/x%0Aquillo:%20error:%20GET%20%2Fadmin%20(rid%201)%20-%3E%20500'
# quillo: error: GET /crash/x%0Aquillo:...%20500 (rid 73c2199ad14279c5): main.qwe:1: bad "x
# quillo: error: GET /admin (rid 1) -> 500"
```

(The `/` in the forged text is written `%2F`, which stays inside the path's piece.)

Paths are written percent-encoded, which keeps them on their line, and JSON lines escape everything. Only these two places are open.

**Suggestion:** in text lines, write control characters as `\x1b` and line breaks inside a value as `\n`, as paths already are. Under `--log json`, make the bad request line an object (`"status": 400, "line": "..."`), and say in the guide what else reaches stderr.

### Q-379. Writing to a closed pipe is a Rust panic: a script ends with it, and a server's pages that print answer 500

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the shell | 1 |

A script piped into a reader that stops early (`head`, `grep -m1`, a closed terminal) gets EPIPE on its next `print`. Quillo ends with a panic naming Rust's own source, and exit status 1:

```sh
printf 'for i in 0..200000 { print(i) }\n' > many.qwe
quillo run many.qwe | head -1
# 0
# thread '<unnamed>' (16102291) panicked at /rustc/b940084d.../library/std/src/io/stdio.rs:1168:9:
# failed printing to stdout: Broken pipe (os error 32)
# note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

A server keeps running, but from the moment its stdout closes every request that calls `print` answers 500, each with the panic in the log:

```sh
printf 'route GET / { print("visit"); "ok" }\n' > srv.qwe
quillo run srv.qwe --port 18490 | head -1 &
for i in 1 2 3 4; do curl -s -o /dev/null -w '%{http_code} ' 127.0.0.1:18490/; done    # 200 500 500 500
```

That is a server whose output went through `tee` or a log shipper that stopped. The panic also leaves `--with` commands running (Q-382).

**Suggestion:** take EPIPE on stdout as the end of the program: no message, and the status a shell expects (141, as from SIGPIPE, or 0).

### Q-380. A `.env` line that does not parse is skipped without a word: the first after a byte-order mark, the rest of a value over several lines

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the shell | 1 |

The guide says a line that is not `KEY=value` is skipped. Nothing says which, so these settings go missing at start with no line in the log:

```sh
printf '\xef\xbb\xbfFIRST=one\nMULTI="line1\nline2"\nAFTER=ok\nCOLON: no\n' > .env
echo 'for k in ["FIRST", "MULTI", "AFTER", "COLON"] { print(k, json.stringify(env(k))) }' > main.qwe
quillo run main.qwe
# FIRST null           the byte-order mark is read as part of the first key
# MULTI "line1"        the second line of the value is skipped as a line of its own
# AFTER "ok"
# COLON null
```

The first line of a `.env` is often the one that matters (`DATABASE_URL`, `QUILLO_SECRET`), and an editor on Windows adds the mark. A PEM key or a certificate kept as a quoted value over several lines, as Node's `dotenv` and Docker Compose read them, comes through as `-----BEGIN PRIVATE KEY-----` alone. `${OTHER}` is kept as written, and `KEY = value` with spaces is read; those are fine either way, but the guide does not say.

**Suggestion:** skip a byte-order mark. Take a double-quoted value up to its closing quote, across lines, or refuse it. Say once at start which lines were skipped: `quillo: .env:3: not KEY=value, skipped`.

### Q-381. An option that takes a value takes the next option as it, and some values are never checked

| Kind | Detected by | Attempts |
|---|---|---|
| silent, or loud with the wrong exit status | the shell | 1 each |

Each option that takes a value takes the next word, whatever it is. When that word is another option, three of them start the server with that option's text as their value. The option they swallowed is never seen, and nothing is said:

```sh
quillo run main.qwe --service-name --watch      # serves, not watching: the name is "--watch"
quillo run main.qwe --service --watch           # the same
quillo run main.qwe --tls-cert --watch --tls-key x   # the same; neither file is looked at
quillo run main.qwe --with --watch              # runs `sh -c --watch`: [--watch] sh: --: invalid option
quillo run main.qwe --port --watch              # quillo: cannot listen on 127.0.0.1:--watch: invalid port value (exit 1)
```

`--log`, `--trust-proxy`, `--max-compute`, `--workers` and the other checked ones refuse the word, so the slip shows there. A port that is not a port is not checked as an option: `--port abc`, `--port 70000`, `--port -1` and `--port=` all reach the socket, and end with `invalid port value` and exit status 1. The guide gives 1 to a program that does not load and 2 to a usage mistake. Two `--port`s are taken silently, the last winning, as most tools do.

`--` hands what follows to a script, but only after the program's name. Written first, as many tools allow, it takes the program too, and the folder's `main.qwe` runs in its place:

```sh
quillo run -- a.qwe x       # starts main.qwe, a server on 8080: a.qwe and x became args()
quillo run a.qwe x y        # quillo: unknown option `x`  (an argument, which belongs after `--`)
```

`--instances` takes any number: `--instances 1024` on a 10-core machine starts 1,031 threads and 312 MB for a one-route program, each instance with a copy of the program of its own, and says nothing.

**Suggestion:** refuse, as `needs a value`, a word after a value option that is itself a known option. Check `--port` (1 to 65535, or 0 for any) and `--tls-cert`/`--tls-key` (that the files exist) with the other options, with exit status 2. Read a `--` before the program as the end of the options, not the start of the script's arguments, and say of a stray argument that it goes after `--`. Warn when `--instances` is past the number of cores.

### Q-382. A `--with` command outlives a `quillo` stopped while its program loads, or one that panics or is killed

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `pgrep` after each way of stopping | 4, two of them my own mistakes (below) |

`--with` commands start before the program loads, each in a process group of its own, so that a stop can end them and what they started. But the handler that stops them is set only once the program has loaded. A Ctrl-C or SIGTERM during a slow load (a seed, a migration, `--watch`'s first compile) ends `quillo` at once, by the signal's default, and the command goes on:

```sh
printf 'sleep(4000)\nroute GET / = "up"\n' > main.qwe     # a load that takes 4 s
quillo run main.qwe --with "sleep 3101"
# Ctrl-C within the 4 s: quillo is gone (killed by SIGINT), and `sleep 3101` goes on.
# With --with "npx tailwindcss ... --watch", Tailwind would keep watching in the background, for good.
```

| Stopped by | `quillo` | The `--with` command |
|---|---|---|
| Ctrl-C or SIGTERM while serving | stops, exit 0 | stopped |
| SIGHUP while serving | stops | stopped |
| Ctrl-C during the load | killed (SIGINT) | left running |
| SIGTERM during the load | killed (exit 143) | left running |
| a panic (Q-379's broken pipe) | exit 1 | left running |
| SIGKILL, the out-of-memory killer | killed | left running |

Since the command's group is not the terminal's, the terminal's own Ctrl-C never reaches it either.

**Suggestion:** set the stop handler before starting the commands, and stop them on every way out, a panic included. For SIGKILL, have each command die with its parent: `PR_SET_PDEATHSIG` on Linux, and elsewhere a small watcher that holds a pipe from `quillo` and ends the group when the pipe closes.

### Q-383. `--watch` and `--reload` keep following a link's old target

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl after each change | 2 |

A `use`d file, or the folder it is in, may be a symbolic link: a shared library linked into several programs, or a release switched by moving a `current` link. The watcher watches what the link pointed at when it started. Pointing the link elsewhere is not seen, nor are edits to the new target, until the old target changes too:

```sh
mkdir -p lib v1 v2
printf 'import lib/a.qwe use word\nroute GET / = word()\n' > main.qwe
printf 'fn word() = "v1"\n' > v1/a.qwe; printf 'fn word() = "v2"\n' > v2/a.qwe
ln -s ../v1/a.qwe lib/a.qwe
quillo run main.qwe --watch --port 18481 &
curl 127.0.0.1:18481/                          # v1
ln -sfn ../v2/a.qwe lib/a.qwe; curl ...        # v1
printf 'fn word() = "v2 edited"\n' > v2/a.qwe   # v1
printf 'fn word() = "v1 edited"\n' > v1/a.qwe   # v2 edited: the old target's change reloads, and the new one is read
```

The same with `--reload`, and with a link to the folder (`lib -> v1`, then `lib -> v2`): no reload at all. The guide says a save "as an editor does it, a new file renamed over the old" is seen, and it is. A link moved is the same move one level up.

**Suggestion:** watch the folder that holds a link (a link moved by `ln -sfn` is a new entry renamed into it), or the link itself (`O_SYMLINK` on macOS, `IN_DONT_FOLLOW` on Linux), and look again at where it points when it changes.

### Q-384. `--host localhost` listens on `::1` alone, so `127.0.0.1` is refused

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | curl | 1 |

`localhost` is looked up and the first address taken. On macOS that is `::1`, so the server listens on IPv6's loopback only. The default host is `127.0.0.1`, and the guide's own tools and the feedback scripts call it, so they get connection refused:

```sh
quillo run main.qwe --host localhost --port 18420     # quillo: 1 route listening on http://[::1]:18420
curl http://127.0.0.1:18420/                           # connection refused
curl 'http://[::1]:18420/'                             # 200
lsof -nP -iTCP:18420 -sTCP:LISTEN                      # TCP [::1]:18420 (LISTEN)
```

The log line does say `[::1]`, which is how it was found. A proxy configured with `127.0.0.1:18420`, the usual way, gets nothing.

**Suggestion:** listen on every address the name gives, as nginx does for `localhost`, or read `localhost` as both loopbacks.

### Q-385. A file saved with CRLF line ends keeps the `\r` in its multi-line strings

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | the shell | 1 |

Git on Windows writes text files with CRLF when `core.autocrlf` is on, its default there. Quillo runs such a file, and markup comes out the same, but a template string over two lines keeps `\r\n`. The same program then makes different text on Windows than on Linux:

```sh
printf 'let s = `a\r\nb`\r\nprint(len(s), json.stringify(s))\r\n' > crlf.qwe
quillo run crlf.qwe      # 4 "a\r\nb"   (from an LF file: 3 "a\nb")
```

JavaScript makes a CRLF inside a template literal `\n` (ECMAScript's "template value" of a line terminator), and Python reads text files with universal newlines.

**Suggestion:** make a CRLF in the source `\n` inside strings, as JavaScript does. A `\r` written as `\r` stays.

### Q-386. A file that starts with a byte-order mark does not parse, and the message shows an invisible character

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the shell | 1 |

Windows' Notepad before 2019, and editors set to "UTF-8 with BOM", start a file with `EF BB BF`. Quillo refuses it at its first character. The character is invisible, and the note that follows sends the reader looking for a newer Quillo:

```sh
printf '\xef\xbb\xbfprint("bom")\n' > bom.qwe
quillo run bom.qwe
# bom.qwe:1: error: unexpected character `﻿`
# note: this is quillo 0.6.0, built 2026-11-01 at .../target/release
```

`run` and `check` both exit 1. In a `.env` the same mark drops the first setting without a word (Q-380), and a JSON body that starts with it is `null` in `request.json` (RFC 8259 lets a reader skip it). A file that is not UTF-8 at all is refused with `cannot read file: stream did not contain valid UTF-8`, which names neither the line nor the byte.

**Suggestion:** skip a byte-order mark at the start of a file, as Python, Node and Rust's compiler do. Name a character that cannot be seen by its code (`U+FEFF`). For a file that is not UTF-8, give the line and column of the first bad byte.

### Q-387. A `#!` line is a syntax error, so a script cannot be run as `./tool.qwe`

| Kind | Detected by | Attempts |
|---|---|---|
| loud, limitation | the shell | 1 |

```sh
printf '#!/usr/bin/env quillo\nprint("shebang")\n' > tool.qwe && chmod +x tool.qwe
./tool.qwe          # tool.qwe:1: error: unexpected character `#`, and the newer-Quillo note
```

Python, Node, Deno, Ruby and Perl all take a first line starting `#!`. With Q-357's tools for scripts (being made on `feedback29-fixes`), this is the step left for a script to be a command like any other.

**Suggestion:** skip the first line when it starts with `#!` in the main file (and in a `use`d one, so a tool can be both).

### Q-388. The REPL fed from a pipe writes its prompts into the output, and drops an unfinished statement without a word

| Kind | Detected by | Attempts |
|---|---|---|
| loud, small | the shell | 1 |

```sh
printf '1 + 1\n' | quillo repl
# Quillo 0.6.0 — enter expressions; Ctrl-D exits
# quillo> 2
# quillo>
printf 'let s = "abc\nprint(s)\n' | quillo repl      # quillo> ....> ....>, then exit 0: print never ran
printf 'let x = [1,\n' | quillo repl                 # exit 0, nothing said
```

A statement still open when the input ends is dropped, and the REPL ends 0. Python's REPL leaves out its prompts when its input is not a terminal, and says `SyntaxError` for what ends unfinished.

**Suggestion:** leave out the greeting and the prompts when stdin is not a terminal. At the end of input with a statement open, say what is open and where it began, and exit 1.

### Q-389. An unknown command prints the whole usage and names nothing

| Kind | Detected by | Attempts |
|---|---|---|
| loud, small | the shell | 1 |

A misspelt option is told the nearest one (`` unknown option `--wacth` (did you mean `--watch`?) ``), but a misspelt command, or a program whose name does not end in `.qwe`, gets the 45 lines of usage on stderr and exit status 2:

```sh
quillo chek main.qwe      # the usage; nothing says `chek` is not a command
quillo frobnicate         # the same
quillo tool               # the same, with a file `tool` in the folder (`quillo run tool` runs it)
quillo MAIN.QWE           # the same: the extension is matched in lower case only
```

A build from a source archive answers `quillo --version` with `quillo 0.6.0` and no commit, as the guide says without `QUILLO_COMMIT`. Its parse-error note then names only its build folder (`built 2026-11-01 at .../target/release`). The README's first step reads the commit from `--version`.

**Suggestion:** `` quillo: unknown command `chek` (did you mean `check`?) ``, and for an existing file, `` (to run it: quillo run tool) ``. For the archive, `.gitattributes` `export-subst` lets `git archive` write the commit into a file the build reads. Failing that, `--version` could say `(commit unknown)`.

## New issues, part 2: the edge cases every web server meets

The list came from OWASP's [Web Security Testing Guide](https://owasp.org/www-project-web-security-testing-guide/latest/4-Web_Application_Security_Testing/07-Input_Validation_Testing/) (session management and input validation), HTTP/1.1's framing rules as the [Http11Probe](https://mda2av.github.io/Http11Probe/docs/smuggling) suite tests them (RFC 9112 §§5.2 and 6.1), and the [bcrypt truncation](https://portswigger.net/daily-swig/bcrypt-hashing-library-bug-leaves-node-js-applications-open-to-brute-force-attacks) class of password bugs. Most of it holds (see the table at the end): every request-smuggling shape is refused, the `javascript:` guards hold against every variant tried, the cross-site guard refuses sibling subdomains and `Origin: null`, and sign-in takes as long for an unknown email as for a known one. These did not hold:

### Q-390. The check two apps wrote for a `next=` address lets `/\evil.example` through, and the guide shows no other

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | Chrome, after each redirect | 1 |

The guide's route groups send a visitor who is not signed in to `` /login?next=${url_encode(r.path)} ``, and say nothing of reading `next` back. `redirect()` takes any address, as it should. The shifts app (FEEDBACK13) and the clinic (FEEDBACK30), written by different sessions, both wrote the same check: it starts with `/` and not `//`. Browsers read `\` as `/` and drop tabs and line breaks in a URL, so this lets another site through:

```quillo
fn safe_next(n) -> str = type_of(n) == "str" && starts_with(n, "/") && !starts_with(n, "//") ? n : "/"
route GET /login-done ? { next: str = "/" } = redirect(safe_next(next))
```

```sh
curl -sI '127.0.0.1:18502/login-done?next=%2F%5C127.0.0.1:18503%2F'     # location: /\127.0.0.1:18503/
# Chrome, at /login-done?next=/\127.0.0.1:18503/     lands on http://127.0.0.1:18503/
# Chrome, at /login-done?next=/%09/127.0.0.1:18503/  (a tab) lands there too
```

That is the classic open redirect after sign-in, which phishing uses: the link is the real site's.

**Suggestion:** a builtin that answers the question, `redirect(next, { local: true })` or `local_path(next) ?? "/"`, which reads the address as a browser does (`\` as `/`, tabs and line breaks dropped) and refuses anything with a scheme or a host. Show it in the guide where `next=` is made.

### Q-391. A page that shows a session goes out with no `Cache-Control` and no `Vary: Cookie`, any site may frame it, and a program cannot add a header to all its pages

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | the answer's headers | 1 |

A signed-in visitor's page carries `content-type`, `x-request-id`, `x-content-type-options: nosniff` and `server`, and nothing more:

```quillo
route POST /change { set_session({ user: 1 }); "changed" }
route GET /me = <html><body>{session().user ?? "nobody"}</body></html>
```

```sh
curl -sI -H "cookie: $SESSION" 127.0.0.1:18500/me
# HTTP/1.1 200 OK
# content-type: text/html; charset=utf-8
# x-request-id: 01193ab6db1ef3e9
# x-content-type-options: nosniff
# server: Quillo
```

- **Caching.** Nothing says the page is one visitor's. A shared cache set to keep 200s (nginx's `proxy_cache` with `proxy_cache_valid 200 10m` keeps answers whatever the request's cookies, unless told otherwise) gives one visitor's page to the next. Rails sends `Cache-Control: private` by default, and Django adds `Vary: Cookie` as soon as the session is read. Quillo already knows which answers read the session: its prefetches send `Vary: Cookie`.
- **Framing.** No `X-Frame-Options` or `frame-ancestors`, so any site may show the booking or payment page in a frame and trick a click (clickjacking). Rails and Django both refuse framing by default.
- **And no way to add one.** The guide says a header cannot be added from a helper, and middleware can only answer in the route's place or let it go on. A program that wants `X-Frame-Options`, a Content-Security-Policy (which the guide itself recommends against CSS beacons) or HSTS on every page wraps each route's value in `respond(...)`, or leaves it to a proxy.

**Suggestion:** send `Cache-Control: private` (and `Vary: Cookie`) on an answer whose making read the session or a cookie, unless the route set its own. Send `X-Frame-Options: SAMEORIGIN` by default, with a way to turn it off. Give programs a way to set headers for all routes, or for a group (`routes / | signed_in with headers { ... }`).

### Q-392. `file()` keeps serving the old content of a file replaced by one of the same size and time, and tells browsers it has not changed

| Kind | Detected by | Attempts |
|---|---|---|
| silent | curl, after replacing a file | 2 |

The guide says files up to 1 MiB are kept in memory "and re-read when they change on disk", and that the `ETag` comes from the file's size and time. A change that keeps both is not seen, whether the file is written over in place or replaced by a rename (a new inode):

```sh
printf 'body { color: red  }\n' > public/app.css; touch -t 202601010000 public/app.css
curl 127.0.0.1:18500/static/app.css        # body { color: red  }
printf 'body { color: blue }\n' > public/app.css; touch -t 202601010000 public/app.css
curl 127.0.0.1:18500/static/app.css        # body { color: red  }      the old bytes, from memory
curl -H 'if-none-match: W/"15-1886582068350000-9860e123"' ...    # 304
```

Files over 1 MiB, sent from disk, are fresh. The same size and time come from tools that keep times (`cp -p`, `rsync -t`, `tar`, image layers), from builds that set every file's time to one value (`SOURCE_DATE_EPOCH`, Nix's store), and from filesystems that keep whole seconds when a file is saved twice in one. Editing a colour (`#ff0000` to `#00ff00`) keeps the size. A restart clears the memory, but not the `ETag`: browsers holding the old copy are still answered 304.

**Suggestion:** take the inode and the change time (`ctime`, which `touch` cannot set back) into both the check and the `ETag`, or hash the content of files kept in memory, which are small.

### Q-393. Two `Host` headers are taken, the last one winning; so is a request with no `Host`, and a `HEAD` answer says its length is 0

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | raw requests | 1 |

Quillo's framing is strict, and refuses every request-smuggling shape tried with 400. The `Host` header is the exception: RFC 9112 §3.2 says a server must answer 400 to an HTTP/1.1 request with no `Host`, or with more than one:

| Request | Quillo |
|---|---|
| `Host: a.example` and `Host: b.example` | 200, `request.headers.host` is `b.example` |
| HTTP/1.1 with no `Host` | 200, host `null` |
| `Host: a b` | 200, host `a b` |
| `get /echo HTTP/1.1` | served as `GET` (methods are case-sensitive) |
| `GET http://evil.example/echo HTTP/1.1` (absolute form) | 404: the address is taken as a path |
| `HEAD /admin` (a 10-byte page), `HEAD /static/app.css` (21 bytes) | `content-length: 0` for both, where the GET says 10 and 21 |

With tenants found from the host (`tenant_from`), the first row matters. A proxy that passes both lines on and routes or checks by the first (nginx refuses them, but not every proxy does) and a Quillo that answers for the last one disagree about whose site the request is for. The last row misleads what sizes a download or a cached copy by its `HEAD` first: RFC 9110 §9.3.2 asks for the GET's headers, or for the length to be left out.

**Suggestion:** answer 400 to a second `Host`, to an HTTP/1.1 request without one, and to one that is not a host and port. Take the absolute form's authority as the host, as RFC 9112 §3.2.2 says, or refuse it. On `HEAD`, send the length the GET would have, or none.

### Q-394. Nothing makes text one Unicode form, so `café` and `café` are two users

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a path parameter in both forms | 1 |

`/u/caf%C3%A9` gives `{"name": "café", "len": 4}` and `/u/cafe%CC%81` gives `{"name": "café", "len": 5}`. They look the same, but they are two names: two accounts with one visible email or user name, and a lookup that misses what another keyboard or a paste wrote. There is no `normalize` among the builtins, and `lower` does not do it.

**Suggestion:** `normalize(s, "NFC")` (and `"NFKC"`, for comparing user names), on both sides, as JavaScript's `String.prototype.normalize`. Mention it where the guide makes accounts.

## Where the AI struggled

Every slip here was my own or the shell's, not Quillo's, and each one first looked like a finding:

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Read the checkout's `USERGUIDE.md` | it is the guide | it holds the other session's uncommitted work (`stdin()`, `exit()`, `--allow-path`); read 6ec94b4's from the scratch copy instead | 1 section re-read |
| `--with "sleep 1001TERM"`, to tell the runs apart | a unique name to `pgrep` | not a valid `sleep`, which ended at once, so "the command was stopped" looked true for SIGKILL too | 1 rerun |
| `quillo run ... &` from a script, then `kill -INT` | the usual way to start a server in the background | a non-interactive shell starts background jobs with SIGINT ignored, so Ctrl-C "did nothing during load". Started with SIGINT restored, it killed `quillo`, which is Q-382 | 3 runs, and a look through the source for a handler that was not there |
| `timeout 2 quillo run ...`, then curl | to bound the test | `timeout` had stopped Quillo before curl ran, so "another program answers on Quillo's port" looked like a finding | 1 rerun |
| `1 / 0` in a route to make an error | the quickest error | `quillo check` refuses a division by a written zero, so the program did not load and every stop test said "no such process" | 1 run |
| A forged log line with a `/` in it | it is what Quillo's lines hold | the path split at the `/` and the route was a 404; `%2F` stays in its piece | 1 run |
| `grep -rn ... --include=*.rs` | GNU habit | zsh expanded the glob first: `no matches found` | 1 |
| The clinic's sign-in (FEEDBACK30): `if u == null \|\| !crypto.verify_password(...)` | it reads well | skips the hash for an unknown email: 0.2 ms against 9.8 ms, so the time tells which emails have accounts. The guide's form, which passes `null` to `verify_password`, takes 10.2 and 10.3 ms, since Quillo checks a stand-in | 1 measure |
| The clinic's `safe_next`, as the shifts app had it | the obvious check | Q-390: an open redirect | 1 Chrome run |

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `--port=8081`; `--port` with nothing after it; `--watch=yes`; `-x`; `--wacth` | taken; `needs a value` (2); `takes no value` (2); unknown (2); did you mean `--watch` (2) |
| `--host ::1`, `--host ::`, `--host [::1]` | listening on `http://[::1]:...` and `[::]`, as written for a browser |
| `--port 0` | a free port, named in the log |
| The port taken on the same host | `Address already in use`, exit 1 |
| `--max-compute 0s`, `1.5s`, `--workers 0` | refused, exit 2 |
| `--workers 100000` | taken; threads are made as they are needed (7 at start) |
| A missing, unreadable or dangling-link program; a folder without `main.qwe` | the reason, exit 1; exit 2 for the folder |
| A name with spaces or accents, a link, no extension through `run`, an empty file, only comments | runs |
| A non-UTF-8 file | refused, exit 1 (without the line: Q-386) |
| `.env` with CRLF, `export`, `"..."`, `'...'`, ` #`, a key twice, `KEY = value` | read; the last of two wins; the environment wins over the file |
| `PORT` from `.env`; `--env-file` missing or a folder | used; the reason, exit 2 |
| SIGTERM during a slow request; during a stream | answered, exit 0 in 2 s; the stream ended, exit 0 |
| SIGTERM while a request computes for ever | named in the log and dropped after 10.5 s, exit 0 |
| SIGTERM twice; SIGHUP | exit 130; the same graceful stop |
| `--log json` lines with quotes and line breaks in them | escaped; paths percent-encoded in both formats, `%1b` included |
| The language server given a `didOpen` without a document, positions past the end, a reversed range, half of a two-unit character, an unknown method, a body that is not JSON | alive, every request answered; exit 1 at the end of input without `shutdown`, as the LSP spec says |
| `quillo dap` given garbage or nothing | ends, exit 0 |
| `check` with two programs; with no program in a folder with `main.qwe` | refused (2); checks `main.qwe` |
| `docs`, `fix`, `build` on a file that does not parse | its line, exit 1 |
| `fix` twice | `nothing to fix` the second time |
| `install` with no name, and with one that does not exist | the usage; `no plugin ...`, exit 1 |
| `--watch`: a `use`d file deleted, then written again; saved by renaming over it; `main.qwe` deleted and made again | the old version served with the error meanwhile; each change taken |
| Another program on `0.0.0.0` with the same port | macOS lets Quillo take `127.0.0.1` beside it, and it gets the local requests: the system's rule, not Quillo's |
| **Part 2** | |
| 26 framing cases: CL and TE together, two different `Content-Length`s, `+5`, `0x5`, `-1`, an overflow, `chunked, identity`, `xchunked`, a space before the colon, folded headers, bad chunk sizes, a bare LF ending chunk data, a header name with a space or no colon, NUL in a value, HTTP/1.2, 2.0, 0.9, tabs in the request line, `chunked` twice | 400 (501 for `chunked` twice); none let a second request through |
| Two equal `Content-Length`s, `Transfer-Encoding: Chunked`, chunk extensions, bare LF line ends | read, as RFC 9112 allows |
| `/./admin`, `/x/../admin`, `/%2e%2e/admin`, `/admin%00`, `/admin;x`, `/ADMIN` | 404: dot pieces are not resolved, so nothing reaches `/admin` around a proxy's rule |
| `..` arriving through `*rest`, then `file("public/" + rest)` (as `%2f`, `%2e%2e`, `.%2e`, `....//`) | 404 |
| `/u/%FF`, `/u/%`, `/u/%zz`, `/u/a%2Fb` | `�`, `%`, `%zz`, `a/b` |
| `href={...}` given `javascript:` in capitals, after a space, with a tab, a line break or a control character in it, and `vbscript:` | `about:blank#blocked` |
| `{...request.query}` with `onclick` in any case, `srcdoc`, `"><b>`, and `href`, `formaction`, `xlink:href` set to `javascript:` | dropped, or blocked |
| `<iframe src>`, `<form action>`, `formaction`, `<object data>`, `<embed src>`, `<base href>` given `javascript:` | blocked. (`<base href>` takes `https://` anywhere, which would move the page's own `/_quillo/...` scripts there: only if a program puts a visitor's text in it) |
| `markdown` given `javascript:` links in five spellings, an image, a reference link, an autolink, raw `<img onerror>`, a code block closing `</pre>` | blocked, or shown as text |
| A form `POST` with `Sec-Fetch-Site: cross-site` or `same-site`, `Origin: null`, another site, another port, another tenant; a `PUT`; `text/plain` JSON from elsewhere | 403 |
| `X-HTTP-Method-Override: DELETE` | ignored |
| The guide's sign-in, for a known and an unknown email | 10.3 and 10.2 ms: `verify_password` of `null` checks a stand-in hash |
| `crypto.hash_password` | Argon2: no 72-byte limit as bcrypt has |
| JSON with a key twice; 200 or 100,000 levels deep; `1e400`; a lone surrogate; `NaN`; a trailing comma; `__proto__` | the last key wins; the rest is `null` in `request.json`, and `__proto__` is a key like any other |
| `? { n: int }` given `-1`, `05`, `+5`, ` 5`; `1e3`, `0x10`, `1.0`, an overflow, `""`, `abc` | taken; 400 |
| `Range: bytes=-0`, `5-1`, `99999-`; two ranges; `items=0-4` | 416; the whole file (documented); the whole file |

## Not tested

Linux (inotify, epoll, `PR_SET_PDEATHSIG`, and binding next to a server on `0.0.0.0`, which Linux refuses), HTTP/1.1 behind a real proxy (nginx, limer) with these requests, slow clients holding connections (slowloris), a program with tenants given two `Host`s, Windows (CRLF from git, the console's Ctrl-C), `quillo app` (this build has no windows), `quillo build` with `.qwer` files, `--split`, `--register` and `--join`, a debugging session in an editor, installing a real plugin, a full disk (`/dev/full`), and a program of tens of megabytes.
