# Feedback from reviewing what Quillo needs to run on cPanel, with an allowlist of plugins and a sandbox

| | |
|---|---|
| App | None. The question: what would it take to support cPanel hosting (a shared account, no root, Apache or LiteSpeed in front), with whoever runs Quillo able to say which plugins a program may use, and with the program sandboxed? What is there today, what is missing, and in what order to build it |
| Quillo | 0.6.0 (5a919f0, 2026-11-02): the release build in `target/release` on `feedback27-35-fixes`. The working tree also has another session's uncommitted edits to `own_file` in [src/builtins.rs](../../src/builtins.rs); none of the results below depend on them |
| Model | Claude (Opus 5.5) |
| Tested with | Small programs in a scratch folder: a link inside the program's folder pointing out of it, read, written, deleted, served and uploaded through; a `.qwer` plugin built with `quillo build`, then run with no cargo, changed with no cargo, and replaced by another library under its name; a `.qwer` call and `save_upload` inside a `with time` function; `--host unix:...` and `PORT=<path>`; threads and memory of an idle server. Read: [src/native.rs](../../src/native.rs) (`build`, `open`, `use_file`), [src/builtins.rs](../../src/builtins.rs) (`local_path`, `allowed_path`, `save_upload`, `env_var`), [src/ast.rs](../../src/ast.rs) (`builtin_effect`), [src/httpd.rs](../../src/httpd.rs) (`Pool::submit`), [src/install.rs](../../src/install.rs), USERGUIDE.md (Rust, Plugins from the catalog, Effects, Scripts from users, Scripts as tools). cPanel's and CloudLinux's documentation for the hosting side (sources at the end) |

No Quillo source was changed. `feedback/INDEX.md` and `feedback/repros/` were not touched; each repro is in its section and the probe scripts are at the end. Numbers start at Q-475, after FEEDBACK40.md's Q-474.

What worked well: much of what a sandbox needs is already there. Quillo has an effect system (`with db, http, fs, env, time, random, net`), enforced by `quillo check` and at run time, which narrows what everything called may do. `run_script` already runs users' code with no effects, a step budget and a size limit. Paths with `..` or absolute paths are refused unless `--allow-path` names the folder, and `--allow-path` follows links to where they really lead. A plugin built once with `quillo build` runs on a machine with no Rust at all, and a plugin that has changed and cannot be built says exactly what to do. `quillo install` checks each file's SHA-256 and version against the catalog. A server reads `$PORT` and listens on 127.0.0.1 by default, which is what a process manager in front of it expects. An idle server is 7 threads and 12.5 MB.

## The answer

Quillo cannot run on most cPanel accounts today, and it has no allowlist or sandbox a host could rely on. Four things are in the way, in this order:

1. **The binary does not start** on the systems most cPanel servers run. The Linux release needs glibc 2.34 (Q-424). AlmaLinux 8 and CloudLinux 8 have glibc 2.28, and cPanel supports AlmaLinux 8 until March 2029. AlmaLinux and CloudLinux 9 (2.34) and 10, and Ubuntu 22.04 and 24.04, are fine.
2. **Nothing can start it and pass it requests the way cPanel does.** On a shared account, an app is started by Passenger (cPanel's Application Manager, or CloudLinux's Node.js and Python selectors), or by Apache's `mod_fcgid`, and is never one long-running process you start yourself. Quillo listens only on a TCP port, and on a shared server 127.0.0.1 is shared by every account (Q-479).
3. **Plugins need cargo, or a library nobody checks.** A cPanel account has no Rust. A library built elsewhere is loaded by its file name alone, so an allowlist of plugin sources means nothing until the library itself is checked (Q-477). A static musl build, the usual answer to item 1, cannot load plugins at all (see Part A).
4. **The sandbox has holes, and no one but the program sets it.** Links let the file builtins out of the program's folder (Q-475). A plugin call and `save_upload` pass any `with` (Q-476). Nothing lets whoever runs Quillo cap what the program may do: there are no plugin, effect, host or variable limits from outside the program (Q-478).

Part A covers what a cPanel account gives and how Quillo could fit it. Part B is a design for the plugin allowlist. Part C is a design for the sandbox. The issues found on the way follow, then a proposed order of work.

One point holds through all three parts. **On shared hosting, the boundary between accounts belongs to the operating system**, through CloudLinux's CageFS and LVE limits or plain Unix users. It does not belong to Quillo. A user who can upload a file can usually upload a binary of their own and run it, so a policy enforced inside `quillo` binds only a `quillo` the host installs and starts itself. What Quillo can do is protect a site from its own plugins and its own bugs, refuse to hand neighbours an open door (a shared 127.0.0.1 port, links out of the folder), and give hosts a binary they can install and configure with confidence. The threat table in Part C says which layer protects whom.

## Part A: cPanel

### What an account gives

| | On a cPanel account | Quillo today |
|---|---|---|
| System | AlmaLinux or CloudLinux 8, 9 or 10, or Ubuntu 22.04 or 24.04; glibc 2.28 on the 8s | needs glibc 2.34 (Q-424) |
| Root | none; no systemd units, no `/etc` | `quillo-website.service` on the VPS is a systemd unit |
| Front | Apache (often with NGINX in front of it) or LiteSpeed, owned by the host; the user writes `.htaccess` | none; expects to be proxied to, or to face the internet |
| Starting an app | Application Manager (Passenger; Node.js, Python or Ruby only), CloudLinux's Node.js and Python selectors (Passenger too), `mod_fcgid` for FastCGI, CGI; long-running processes started by hand or by cron are often killed | `quillo run`, kept up by something else |
| Ports | 127.0.0.1 is the server's, shared with every account; high ports may be firewalled | TCP only (Q-479) |
| Limits (CloudLinux LVE) | CPU, memory (PMEM), processes (NPROC, 100 by default in one CloudLinux article), entry processes (EP, 20 by default: concurrent requests; over it, visitors get 508), I/O, inodes | 7 threads idle, up to 72 waiting (Q-480); 12.5 MB idle |
| Rust | none | `use "x.qwer"` runs cargo when no library is built |
| Disk | a quota, and an inode quota | `~/.cache/quillo/native` grows without end ("clear it now and then") |
| Scheduled work | cron, in cPanel's Cron Jobs page | `schedule` and `every` inside the running server |

Passenger can run apps in any language (its "generic language" support starts a command with `$PORT`). Its documented configuration for that is the `passenger start --app-start-command` command line and `passenger_app_start_command` for NGINX. I found no documented way to set it from `.htaccess` or in cPanel's Application Manager, which lists Node.js, Python and Ruby only. Treat Passenger for Quillo directly as something a host would have to set up, not something a user can do.

### Ways to run on cPanel, best first

1. **FastCGI, `quillo fcgi`.** `mod_fcgid` is in EasyApache 4 and on many shared hosts. The user writes two `.htaccess` lines and a one-line `app.fcgi` that runs `quillo fcgi main.qwe`. Apache starts it, keeps it up, passes it the listening socket on file descriptor 0 (the FastCGI convention), and stops it when idle. No port, no neighbour can reach it, nothing to keep alive. Quillo would need a FastCGI listener beside `httpd.rs`'s HTTP one: the records are simple (begin, params, stdin, stdout, end). What it cannot do: WebSocket (channels, live data over a socket) does not pass through FastCGI. Live data falls back to its HTTP path, streaming would need `mod_fcgid`'s output buffer set to 0, and WebTransport is out. `--instances` makes no sense there: `mod_fcgid` starts more processes itself, so state in globals is per process, as with `--instances`.
2. **Passenger through a shim**, for hosts with Application Manager. A Node.js app of about 30 lines, registered as a Node app, starts `quillo run` on a free port (later, a Unix socket), and pipes each request and WebSocket upgrade to it. It works with nothing new in Quillo, but every request takes one extra hop through Node, Node's memory counts against the LVE, and the quillo port is still on the shared 127.0.0.1 until Q-479 is done. Good as the documented stopgap.
3. **A Unix socket behind `mod_proxy`**, for hosts that set it up (a VPS with WHM, or a "Quillo" feature a host offers). `quillo run --listen unix:/home/u/.quillo/app.sock`, mode 0600, and the host's `ProxyPass` to it. This is the cleanest for a host, needs Q-479, and is what a WHM plugin would configure.
4. **Not recommended:** `@reboot` cron plus `nohup quillo run`, and a `RewriteRule ... [P]` to 127.0.0.1. Many hosts kill long-running processes, `[P]` needs `mod_proxy` allowed in `.htaccess`, and the port is open to every other account.

A guide page "Quillo on cPanel" can document 2 now and 1 once it is built.

### The glibc problem, and why musl is the wrong fix

The guide's "Not built yet" lists "Linux builds linked with musl, or static". A fully static musl binary starts anywhere, but musl's `dlopen` in a static binary does nothing, so it cannot load a `.qwer` library. The plugins would stop working exactly where Part B needs them most. Better: build the glibc release against an old glibc, for instance with `cargo zigbuild --target x86_64-unknown-linux-gnu.2.17` or in a manylinux2014 container. It starts on every system above and still loads libraries. `quillo build` would then need the same baseline for plugins (a plugin linked against glibc 2.39 does not load on 2.28). The catalog's prebuilt libraries (Part B) would be built once, on that baseline.

### The rest of the account

- **Threads.** Linux's process limit counts threads, and an LVE's NPROC is for the whole account: PHP, cron, SSH and Quillo together. A Quillo server grows to `--workers` + 1 wait threads (65 by default), plus about seven of its own, plus one per tick. With the default of 100, a site under load and a few PHP processes reach it. See Q-480. A host-side default of `--workers 8` or so, and a server that reads `RLIMIT_NPROC` and warns when `--workers` would cross it, keep this in hand.
- **Entry processes** limit concurrent requests through Apache, so Quillo's own concurrency is capped in front of it. That is fine for Quillo, but worth one line in the guide: a 508 page is LVE, not Quillo.
- **The cache.** `QUILLO_NATIVE_CACHE` on an account should point inside the account, and with Part B's prebuilt libraries there is no cargo cache at all. Nothing else in Quillo writes outside the program's folder.
- **Jobs.** `schedule` and `every` run inside the server. Under FastCGI or Passenger the process may be stopped when idle, so a job due at 03:00 does not run. Either the guide says to run jobs from cron (`quillo run jobs.qwe`, a script), or `quillo jobs main.qwe --once` runs the jobs that are due and ends, for a cron line every minute.
- **Settings.** cPanel has no place for environment variables of a FastCGI app. The `.env` beside the program works, and `--env-file` covers a file outside the public folder.

## Part B: an allowlist of plugins

### What there is

| | Today |
|---|---|
| Where a plugin comes from | any `.qwer` beside the program, or `quillo install` from the catalog |
| What `install` checks | SHA-256 and `//! version:` against `index.json`, which comes from the same server as the files: anyone who can change one can change both |
| What `use` does | builds with cargo when there is no library for the file's hash; a crate's build scripts run then, on the server, at load |
| What loading checks | that the file name has the hash of the source, the glue and the processor, and that the library exports Quillo's ABI 1 with the same number of functions ([native.rs:905](../../src/native.rs)). Not what is inside it (Q-477) |
| Who can say no | nobody: no flag, no setting, no file |

### What it should do

Two people may want an allowlist, for different reasons:

- **The site owner**, against their own supply chain: only these plugins, at these versions, built from this source. No surprise cargo build on a production server.
- **The host**, offering Quillo on its servers: only catalog plugins it has approved, prebuilt, and no plugin with effects it does not allow (no raw sockets, say).

Both fit one file in the same shape, read from two places. The host's, `/etc/quillo/policy.toml`, comes first and cannot be widened. The program's, `quillo.policy` beside `main.qwe`, can only narrow it:

```toml
# /etc/quillo/policy.toml: what any program on this server may do
[plugins]
build = false                     # never run cargo when loading; prebuilt libraries only
local = false                     # no .qwer of the program's own, catalog plugins only
allow = ["csv@1", "qr", "jwt@2.1.0", "markdown"]
catalog_key = "ed25519:4f7a...e1" # what the catalog's entries must be signed with

[effects]
allow = ["db", "http", "fs", "time", "random"]   # no env (but the names below), no net

[fs]
links = "inside"                  # a link must lead inside the program's folder

[http]
private = false                   # no 127.0.0.0/8, 10/8, 172.16/12, 192.168/16, 169.254/16, ::1, fc00::/7
hosts = ["*"]

[env]
names = ["QUILLO_SECRET", "STRIPE_KEY", "DATABASE_URL"]

[limits]
workers = 8
max_compute = "10s"
max_body = "10MB"
```

```toml
# quillo.policy, beside main.qwe: this program narrows it further
[plugins]
allow = ["csv@1.4.2", "qr@1.0.3"]
[http]
hosts = ["api.stripe.com", "*.mapbox.com"]
```

What each part means:

- **A plugin is named by the catalog's name and version, and pinned by hashes.** `quillo install` already writes the source; it would also write `.quillo/plugins.lock` with each plugin's version, the source's SHA-256 and, for each processor and baseline it was built for, the library's SHA-256. `quillo build` adds the library's hash for a local plugin. Loading checks the library's hash against the lock, which closes Q-477. Hashing a few megabytes once at start costs a few milliseconds.
- **The catalog signs its entries.** `index.json` gets an Ed25519 signature over each entry (name, version, source hash, library hashes, declared effects), made offline with a key whose public half is in the binary and may be pinned in the policy. A server that can change `index.json` then still cannot make a plugin pass.
- **The catalog ships prebuilt libraries** for the baseline in Part A, linux-x86_64 and linux-aarch64 first. With `build = false`, the catalog's library is the only way in, so cargo is never needed or run on the server.
- **A plugin declares its effects**, in its header: `//! effects: http, fs`. The catalog reviews the claim when it publishes and signs it. A policy that does not allow `net` refuses any plugin that declares it, at `quillo check` and at load. A local plugin with no declaration has every effect. Q-476 is the other half: inside a `with` function, a plugin's call needs the effects it declares.
- **What is refused says why and how**, as the cargo message already does: `` `pdf_render` (pdf.qwer 2.0.1) is not in this server's plugin list (/etc/quillo/policy.toml): csv, qr, jwt, markdown ``.

This is honest only up to a point. A native plugin is Rust in the server's process: its declared effects are a reviewed claim, not something Quillo can enforce. The allowlist says *which code* runs, not *what it does*. For the second, see the plugin tier in Part C.

## Part C: a sandbox

### Who is protected from whom

| Threat | Protected by | Quillo's part |
|---|---|---|
| A visitor, against the site | the program, and Quillo's defaults | paths stay in the folder (Q-475), uploads cannot replace the program (Q-361), errors hidden, `--max-compute`, `--max-body` |
| Users' own code, run by the site | `run_script` | has it: no effects, 20,000 steps, 1 MB |
| A plugin or a library, against the site's owner | Part B's allowlist, and the plugin tier below | to build |
| The program, against the host's rules | a policy the host sets, on a binary the host installs and starts | to build: Q-478 |
| One account, against another | the OS: Unix users, CageFS, LVE | not listen on the shared 127.0.0.1 (Q-479); not follow links out (Q-475) |
| The account's owner, against the host | the OS alone | none: a user can run a binary of their own |

### Three layers

**1. The language: effects, set from outside.** The machinery exists. A function's `with` narrows what all its callees may do, at check and at run time. What is missing is a ceiling from outside the program, with parameters:

- `--effects db,http,fs,time` (and the policy's `[effects]`) runs the whole program as if its top level were declared `with` those. Every route, job and component's server code then runs under it. The check already reports a function that needs more, so `quillo check --effects ...` tells an owner beforehand what a host's policy would refuse.
- `fs` gets roots: the program's folder, the `--allow-path` folders, and links resolved, always (Q-475).
- `http` gets hosts, and a `private = false` that refuses loopback, private and link-local addresses after DNS resolution (and on each redirect). On a shared server, 127.0.0.1 is every account's services, and `169.254.169.254` is the cloud provider's metadata.
- `env` gets names. Today `env()` reads any variable of the process: `PATH`, `HOME` and whatever the host's process manager set.
- `net` (UDP, channels) is off unless allowed.

The cost is the check the effect system already makes, done once more per builtin call from the top-level mask. It is a byte comparison and should not show in `bench/vs-python`, but measure it.

**2. The process, on Linux: `quillo run --sandbox`.** After the program has loaded (files read, libraries opened, database open, port bound), Quillo restricts itself:

- `prctl(PR_SET_NO_NEW_PRIVS)`, and a seccomp filter that refuses `execve`, `fork` of new programs, `ptrace`, `mount`, and other calls a web server never needs. `--with`, cargo and `--split` start programs, so they are refused under `--sandbox`, or set up before the filter.
- Landlock (Linux 5.13 and later; AlmaLinux and CloudLinux 9 and 10, Ubuntu 22.04 and 24.04): read and write only under the program's folder, the `--allow-path` folders, the database's folder and `/tmp`; read-only for the system's libraries and certificates. Landlock ABI 4 (Linux 6.7) also limits which TCP ports may be bound and connected to.
- On AlmaLinux 8's 4.18 kernel there is no Landlock: `--sandbox` says what it could not do and goes on with seccomp, or stops with `--sandbox=strict`.
- On macOS, `sandbox_init` with a profile written for the folder, for development parity; Windows has no equivalent worth the work.

This layer turns layer 1's promises into the kernel's for Quillo code. It also covers native plugins, which layer 1 cannot: a plugin's `std::fs::read("/etc/...")` fails under Landlock as Quillo's would. The work is mostly in getting the order right (all loading done before the filter) and in the messages.

**3. Plugins: a sandboxed tier.** Native `.qwer` stays for what needs native code and trust. For the catalog, add plugins built to WebAssembly (`wasm32-wasip2`) and run in wasmtime with WASI capabilities: no files unless granted a folder, no network unless granted hosts, a fuel and memory limit. Values already cross a plugin's edge as JSON, so the ABI stays as it is. Many pure crates (csv, qr, markdown, jwt, image codecs) build to WASI unchanged. Crates that open sockets or start threads mostly do not. The catalog would mark each plugin native or sandboxed, and a host's policy could allow sandboxed plugins freely and native ones by name. It is the only layer that makes a plugin's declared effects enforced rather than claimed. It is also the largest piece of work here, with the cost of a wasm runtime in the binary and a slower call. Measure that against the native call before committing to it.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-424 | The Linux release needs glibc 2.34: it does not start on AlmaLinux or CloudLinux 8, the base of many cPanel servers until 2029. Part A explains why a static musl build would break plugins | 0 (read) |

## New issues

### Q-475. A link inside the program's folder takes `read_file`, `write_file`, `delete_file`, `file` and `save_upload` out of it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe | 1 |

`local_path` ([builtins.rs:1419](../../src/builtins.rs)) refuses `..` and absolute paths, then joins a plain relative path to the program's folder without resolving it. `save_upload` checks its path the same way. A link inside the folder, pointing anywhere, is followed:

```sh
mkdir outside && echo secret-outside > outside/s.txt
mkdir p && ln -s "$PWD/outside" p/data
```

```quillo
// p/main.qwe
print(read_file("data/s.txt") ?? "refused")
print(read_file("../outside/s.txt") ?? "refused ..")
write_file("data/w.txt", "written through link")
```

```
secret-outside
refused ..
```

`outside/w.txt` is written. `delete_file("data/s.txt")` deletes `outside/s.txt`. `route GET /f = file("data/w.txt")` serves it. `save_upload(request.files.f, "data/up.txt")` writes the visitor's upload to `outside/up.txt`.

The guide says paths stay inside the program's folder, and `--allow-path` already judges a path "where a link leads being what counts". The default path does not. A link gets into a folder more easily than it seems: git stores links, so a cloned program can carry `data -> /home/other/...`, and an archive a program unpacks can too. On a shared server, the folder it leads to may be another account's, wherever permissions allow. Without CageFS, world-readable files in other homes are common.

**Suggestion:** Resolve the path (the nearest existing folder above it, as `allowed_path` and `own_file` already do) and require the result to be under the real program folder, or an `--allow-path` folder. A link inside the folder that leads inside it keeps working. On Linux, open with `openat2(RESOLVE_BENEATH)` where the kernel has it (5.6 and later), so a link swapped in between the check and the open is refused too. Give `save_upload` the same check. If a program really wants a link out (shared assets in `public/`), `--allow-path` names it.

### Q-476. A plugin's function and `save_upload` pass any `with`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe | 1 |

The guide says that inside a `with` function, "nothing called can widen" the effects. `builtin_effect` ([ast.rs:133](../../src/ast.rs)) gives a plugin's functions no effect, as for pure builtins, and has no entry for `save_upload` (nor `read_upload`):

```rust
// hi.qwer
pub fn hi(_name: &str) -> String { std::env::var("HOME").unwrap_or_default() }
```

```quillo
use "hi.qwer"
fn pure_one() with time = hi("x")
fn reads() with time = env("HOME")
print(pure_one())
print(reads())
```

```
eff.qwe:3: error: `env` needs the env effect, but `reads` is declared `with time`
```

With the check off, `pure_one()` prints `/Users/restiguay` and only `reads` is stopped. In the same way:

```quillo
fn keep(u) with time = save_upload(u, `uploads/${now()}.txt`)
route POST /up = keep(request.files.f) ?? "saved"
```

`quillo check` says `ok`, and a POST writes `uploads/1793595590380.txt`.

Today this misleads only a reader of `with`. Once effects are the sandbox's first layer (Part C), it is the hole that layer would fall through.

**Suggestion:** `save_upload` is `fs`. A plugin function's effect is what its header declares (`//! effects: http`, Part B), and with no declaration, all of them: a `with` function can call a plugin only once the plugin says what it does. `quillo check` reads the header as it reads the function types. The catalog's 628 plugins would each need the line, which a script can mostly derive from their `needs` and crates and a person then reviews.

### Q-477. A library in `.quillo/native/` is loaded by its file name alone

| Kind | Detected by | Attempts |
|---|---|---|
| limitation (documented), security | a probe | 1 |

`build` ([native.rs:818](../../src/native.rs)) loads the library whose name has the hash of the plugin's source, and `open` checks only the ABI number and how many functions it has. The guide says so: "Nothing checks the library against its source". Any library with the same count of functions loads in its place:

```sh
quillo build main.qwe                 # p2: hi.qwer says "hi <name>"
# p3: another hi.qwer, whose hi() returns $HOME; built, and copied over p2's library
cp ../p3/.quillo/native/libhi-*.dylib .quillo/native/libhi-687dd0b24f29e5cf.dylib
CARGO=/nonexistent quillo run main.qwe
```

```
/Users/restiguay
```

`hi.qwer` still says `format!("hi {}", name)`.

As the guide says, whoever can write `.quillo/native/` can run code in the server, and that is acceptable for a developer's own machine. It is what makes an allowlist (Part B) meaningless: a policy that allows `csv@1.4.2` by its source would load any library named for that source.

**Suggestion:** `.quillo/plugins.lock`, as in Part B: each library's SHA-256, written by `quillo build` and `quillo install`, and checked before `dlopen`. A library with no line in the lock is built again where cargo is allowed, and refused where it is not. Hashing at start costs milliseconds, once per process.

### Q-478. Whoever runs Quillo cannot limit what a program may do

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading the runtime | 0 |

Every limit today is the program's own: `with` on its functions, `run_script` for its users' code. From outside, there is `--allow-path`, which only widens, `--max-compute` and `--max-body`. Nothing lets the person who starts `quillo`, the host or the site owner on deploy, say:

- which plugins may load, or that none may be built (a `use` with no library runs cargo, and with it the crates' build scripts, on the production server at load);
- which effects the program may use at all;
- which hosts `http.*` may reach (loopback and the metadata address included), or which variables `env()` may read (any of the process's today, [builtins.rs](../../src/builtins.rs) `env_var`).

**Suggestion:** Part B's policy file and Part C's first layer: `/etc/quillo/policy.toml` and `quillo.policy`, the second only narrowing the first, with `--effects` and `--no-build` as flags for a single run. `quillo check --policy FILE` says, before a deploy, what the program would be refused. The guide's "Possible or not" gets a line: "Limit what a program may do from outside? Yes, with a policy."

### Q-479. Quillo listens only on a TCP port, which on a shared server every account can reach

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a probe | 1 |

```
$ quillo run main.qwe --host unix:/home/u/app.sock
quillo: cannot listen on unix:/home/u/app.sock:8080: failed to lookup address information: nodename nor servname provided, or not known
$ PORT=/home/u/app.sock quillo run main.qwe
quillo: cannot listen on 127.0.0.1:/home/u/app.sock: invalid port value
```

On a VPS of one's own, 127.0.0.1 is private. On a shared server it is everyone's: any account can connect to the port and talk to the program directly, past Apache. It gets the program's health and metrics routes, and the services that `--split` puts on 127.0.0.1 are reachable too, though their calls are signed. Quillo also cannot be started by `mod_fcgid` or another FastCGI manager.

**Suggestion:** `--listen unix:PATH` (made with mode 0600 and removed on stop; `$PORT` may hold such a path, as some process managers pass), for `mod_proxy` and Passenger. Then `quillo fcgi`, a FastCGI listener on file descriptor 0, for `mod_fcgid` (Part A). Under `--split`, services can use Unix sockets in a folder of the program's, which also takes them off the shared 127.0.0.1.

### Q-480. The wait threads can reach an account's process limit, and a thread that cannot start is not said

| Kind | Detected by | Attempts |
|---|---|---|
| silent (not reproduced) | reading the runtime | 0 |

`Pool::submit` ([httpd.rs:485](../../src/httpd.rs)) starts a new wait thread whenever none is idle, up to `--workers` + 1 (65 by default). When `spawn` fails, it lowers the count and returns: the job stays queued for a thread that already exists, and nothing is logged. A server with no wait thread yet, whose first spawn fails, would leave the request waiting for as long as it takes to time out. On Linux the process limit counts threads, and CloudLinux's NPROC (100 by default in one CloudLinux article) is for the whole account. A Quillo server waiting on 64 database calls is about 72 threads, before PHP, cron or SSH.

I did not reproduce the limit: on macOS, lowering `ulimit -u` low enough would stop the shell first. A CloudLinux account, or a Linux machine with `prlimit --nproc`, would show it.

**Suggestion:** When `spawn` fails, log it once (`quillo: cannot start a thread for waiting work (the account's process limit?); requests wait for the N threads there are`). At start, read `RLIMIT_NPROC` and warn when `--workers` plus Quillo's own threads cannot fit in what is left. Run a job in the loop's own thread as a last resort, rather than leave it queued with no thread to take it.

## Proposed order of work

Smallest and safest first. Each step is useful without the next:

1. **Q-475 and Q-476**: paths resolved and kept inside; `save_upload` is `fs`; plugins need declared effects. Small, and they fix holes that exist today.
2. **A Linux release on an old glibc** (Q-424, rethought): built with zig or manylinux2014 against glibc 2.17, still loading plugins. `quillo build` for the same baseline.
3. **`--listen unix:` and `quillo fcgi`** (Q-479), and the guide page "Quillo on cPanel", with the Passenger shim as the way until FastCGI is in. Q-480's log line and `RLIMIT_NPROC` warning go with it.
4. **`plugins.lock` and `--no-build`** (Q-477).
5. **The policy file** (Q-478): plugins, effects as a ceiling, `fs`, `http` hosts and private addresses, `env` names, limits; and `quillo check --policy`.
6. **The signed catalog with prebuilt libraries**, and `//! effects:` on its 628 plugins.
7. **`--sandbox` on Linux**: seccomp and Landlock after loading.
8. **Sandboxed (WASI) plugins**, if a measurement says the call's cost is acceptable.
9. Later, and only with a host to work with: a WHM plugin that installs `quillo` system-wide with a policy, and a cPanel page to register a program, as Application Manager does for Node.js.

Steps 1 to 4 need no new design decisions. Step 5 needs the file's shape agreed (the one above is a starting point). Steps 7 and 8 need their costs measured against `bench/vs-python` before anything is merged.

## Smaller notes

- `quillo install` checks a plugin against `index.json`, but the two come from the same server: the check catches a broken download, not a changed catalog. Part B's signature is the fix.
- `--with` runs `sh -c`, and is the operator's flag, not the program's. Under `--sandbox` it would have to start before the seccomp filter, or be refused.
- The native cache's default, `~/.cache/quillo/native`, grows on an account with an inode quota. With `build = false` it is not used at all. Without it, `quillo build` could remove the crates it no longer needs.
- `env()` with no policy reads the whole environment, and Passenger and `mod_fcgid` pass the host's variables to the process. A program that prints `env("PATH")` on an error page shows the server's layout. Q-478's `[env] names` covers it.

## Where the AI struggled

- I first looped over flags as `for w in "" "--workers 4"` and passed `$w` quoted. zsh passed `--workers 4` as one argument, the server refused it and exited, and the next line printed 0 threads and a failed `kill`. It is the same mistake FEEDBACK40.md recorded. The idle count (7 threads) comes from the first run, which had no flags.
- `print(delete_file("data/s.txt") ?? "refused")` printed `refused`, and I nearly recorded the delete as refused. `delete_file` gives `null` when it succeeds, so `??` took the default. Listing the folder showed `s.txt` was gone.
- My first upload probe used `uploads()` and `{ form: { f: upload } }`, neither of which exists. `request.files.f`, from the guide, was the way.
- I did not find documentation for setting Passenger's generic start command from `.htaccess` or in cPanel's Application Manager, so Part A treats Passenger for Quillo directly as a host's setup, not a user's.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `read_file("../outside/s.txt")` | refused (`null`), with no `--allow-path` |
| A built plugin, run with `CARGO=/nonexistent` and a `PATH` with no Rust | loads and runs: `hi Ana` |
| The plugin's source changed, no cargo | `cannot build ./hi.qwer: cargo did not run (...). Install Rust (https://rustup.rs), or build it where Rust is with \`quillo build\` and bring .quillo/native along`, exit 1 |
| `env` inside `with time` | refused by `quillo check` and, with `--no-check`, at run time, with the caller named |
| An idle server | 7 threads, 12.5 MB resident; with `--instances 2`, 9 threads, 13 MB |
| `$PORT` and the default host | read; listens on 127.0.0.1 |

## Scripts

Every probe used `Q=~/dev/quillo-app1/target/release/quillo` in a scratch folder, and stopped only the PID it started.

Links out of the folder:

```sh
mkdir -p p1 outside; echo secret-outside > outside/s.txt; ln -sfn "$PWD/outside" p1/data
cd p1
printf 'print(read_file("data/s.txt") ?? "refused")\nprint(read_file("../outside/s.txt") ?? "refused ..")\nwrite_file("data/w.txt", "written through link")\n' > main.qwe
$Q run main.qwe; ls ../outside
printf 'print(delete_file("data/s.txt"))\n' > del.qwe; $Q run del.qwe; ls ../outside
printf 'route GET /f = file("data/w.txt")\nroute POST /up = save_upload(request.files.f, "data/up.txt") ?? "saved"\n' > srv.qwe
$Q run srv.qwe --port 47212 & pid=$!; sleep 1
curl -s 127.0.0.1:47212/f; echo up > u.txt; curl -s -F f=@u.txt 127.0.0.1:47212/up
kill $pid; ls ../outside
```

A plugin with no cargo, and a library swapped in:

```sh
export QUILLO_NATIVE_CACHE=$PWD/cache
mkdir p2 && cd p2
printf '/// Says hello.\npub fn hi(name: &str) -> String { format!("hi {}", name) }\n' > hi.qwer
printf 'use "hi.qwer"\nprint(hi("Ana"))\n' > main.qwe
$Q build main.qwe
CARGO=/nonexistent PATH=/usr/bin:/bin $Q run main.qwe          # hi Ana
cd .. && mkdir p3 && cd p3
printf 'pub fn hi(_name: &str) -> String { std::env::var("HOME").unwrap_or_default() }\n' > hi.qwer
printf 'use "hi.qwer"\nprint(hi("x"))\n' > main.qwe
$Q build main.qwe
cp .quillo/native/libhi-*.dylib ../p2/.quillo/native/libhi-687dd0b24f29e5cf.dylib
cd ../p2 && CARGO=/nonexistent $Q run main.qwe                   # $HOME
```

Threads and memory (macOS; `ps -M` lists threads):

```sh
printf 'route GET / = "hello"\n' > main.qwe
$Q run main.qwe --port 47201 & pid=$!; sleep 1
for i in $(seq 200); do curl -s -o /dev/null 127.0.0.1:47201/; done
ps -M -p $pid | tail -n +2 | wc -l; ps -o rss= -p $pid
kill $pid
```

## Sources

- cPanel's supported operating systems: <https://support.cpanel.net/hc/en-us/articles/1500001216582>
- cPanel's Application Manager (Passenger; Node.js, Python, Ruby): <https://docs.cpanel.net/cpanel/software/application-manager/>
- Passenger's generic language support and `$PORT`: <https://www.phusionpassenger.com/docs/advanced_guides/gls/generic_guide.html>, <https://www.phusionpassenger.com/docs/advanced_guides/gls/ports.html>
- CloudLinux LVE limits and their defaults: <https://docs.cloudlinux.com/cloudlinuxos/limits/>, <https://cloudlinux.zendesk.com/hc/en-us/articles/9299899421084-LVE-limits-default-values-and-recommended-values>
- Entry processes and the 508 page: <https://docs.serversaustralia.com.au/docs/cloudlinux-understanding-lve-limits>
