# Feedback from checking how a provider's policy holds on edge platforms, cPanel and others

| | |
|---|---|
| App | None of its own: a check done at the user's request, 2026-11-21: "check how quillo.policy works on edge computing environment, cpanel and other platforms". The user added during the check: "edge/cpanel and other provider should be able to provide unoverridable defaults to quillo developer/users", and "this could be from global path variable that quillo instance can read" |
| Quillo | The released `quillo 0.8.9 (77ffcd0, 2026-11-20)` on the PATH (= tag v0.8.9). `src/policy.rs`, `src/main.rs`, `src/push.rs`, `src/desktop.rs`, `src/mobile.rs`, the guide and the website's hosting guides were read at this checkout (branch `feedback72-81`, `439837f`), not built |
| Model | Claude (Opus 5.5, `claude-opus-5-5`) |
| Tested with | `quillo check` and `quillo run` on macOS, with curl. The provider's layer is played by `--policy provider.toml`, which goes through the same code as the host's file (`Policy::add`, each layer narrowing the last). `/etc/quillo/policy.toml` itself was not written: that needs root on this machine. There is no Docker here, so nothing ran on Linux, Windows, CageFS or an edge platform; what is said of them comes from the code and from those platforms' documentation, and says so. The programs are kept in `.git/wip/feedback82/` (`provider.toml`, `app/`, `linkapp/`, `dirapp/`, `okapp/`) |

Numbers Q-1280 to Q-1286, from the block Q-1280 to Q-1299, agreed with New Quillo Coder, whose batch on branch `feedback72-81` keeps Q-1274 to Q-1279. This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed. The repros are written in the report.

**How the request is read.**

- **A provider** is whoever runs Quillo programs written by others: a shared host (cPanel, Plesk, DirectAdmin), a managed server, a platform that runs containers or small VMs near the visitor (Fly.io, Cloud Run, Lambda, Cloudflare Containers), or a company's own platform team.
- **An unoverridable default** is a setting the provider puts in force that the developer, and the program's own users, cannot raise, widen or switch off. They may only narrow it. This is what Quillo's policy promises: "Each file only narrows what the ones before it allow".
- **The question** is whether that promise holds on each platform, and for which settings. The user's suggestion, a global path variable the instance reads, is weighed in Part C.

What worked well (measured, `app/main.qwe` under `provider.toml`):

- **Numbers hold.** The developer passed `--max-compute 60s`, `--max-body 100MB` and `--workers 64`. A request that spun was ended at 2.03 s with a 500, a 2 MB body got 413, and the start line said `workers 4; max_compute 2000ms; max_body 1048576 bytes; max_memory 67108864 bytes`.
- **The developer's own file only narrows.** Their `quillo.policy` asked for every host (`hosts = ["*"]`) and the `net` effect. The combined policy kept the provider's single host and no `net`, without an error: `example.com is not among the hosts provider.toml allows: api.example.com`.
- **The cloud's metadata address stays closed:** `169.254.169.254 is not among the hosts provider.toml allows`.
- **The provider's environment beats the developer's `.env`.** With `DATABASE_URL=from-provider` in the environment and `DATABASE_URL=from-dotenv` in `.env`, `env("DATABASE_URL")` gave `from-provider`. `env("HOME")` was hidden, as `[env] names` said.
- **Push is held to it too.** Each push message is checked against the policy's hosts as it is sent (`src/push.rs`, `deliver`), so `QUILLO_PUSH_LOCAL` does not get past it.
- **A host file that is there but cannot be read stops the program**, rather than being skipped (`src/policy.rs`, `load`).

## The answer

**The policy holds what it can say, but it can say too little, and it is found in only one place. So a provider can set unoverridable defaults only on a Linux server it controls as root, and only for 11 settings.**

| | Holds? |
|---|---|
| The 11 things a policy says: plugins (build, allow), effects, http (hosts, private), env names, limits (workers, max_compute, max_body, max_memory, max_tool_calls) | ✓ measured: the developer's flags and own file only narrow them |
| Every other option of `quillo run` | ✗ a provider cannot even write them. `--trust-proxy 0.0.0.0/0` let a forged `X-Forwarded-For` become `request.ip`, and `--allow-path /etc` let `read_file` read `/etc/hosts` (Q-1280) |
| Where the provider's file is | ✗ only `/etc/quillo/policy.toml`. An absent file means no policy, silently. On Windows it means `\etc` on the current drive (Q-1281, Q-1282) |
| The program's own `quillo.policy` | ✗ skipped without a word when it is a broken link or a folder (Q-1283, measured) |
| Desktop and phone apps | ✗ no policy at all, not even the program's own (Q-1284, read in the code) |
| Plugins | by file name only: a renamed plugin passes (Q-1285) |

By platform, from the code and the platforms' documentation (not run here):

| Platform | Who controls the binary, the start line, the environment, `/etc` | Does the provider's file reach the program? | Can the developer step around it? |
|---|---|---|---|
| A server of one's own (systemd) | the same person | yes | it is their own server |
| A managed server (the host runs the unit) | the host | yes | no, if the host owns the unit, `/etc` and the binary |
| cPanel or DirectAdmin on CloudLinux (CageFS) | binary: the host's or the developer's upload; start line and environment: the developer's `app.fcgi`; `/etc`: the host's, as each account sees it | **only if the host adds `/etc/quillo` to CageFS**. CageFS gives each account its own view of `/etc`, and a folder outside it is not seen (Q-1281) | yes, by uploading their own `quillo`, unless the home folder is mounted `noexec` |
| cPanel without CageFS, or Plesk | as above, `/etc` shared | yes | yes, by their own binary |
| Containers or microVMs: Fly.io, Cloud Run, Lambda (container image or custom runtime), Render, Railway, Koyeb, Cloudflare Containers | the developer builds the image, so all of it is theirs | only a file they put there themselves | yes: the image is theirs. The provider holds only the VM: CPU, memory, network |
| A managed Quillo runtime (the provider builds the image and runs the start line; the developer gives code only) | the provider | yes, if the provider places the file | no |
| Isolate edges: Cloudflare Workers, Vercel Edge Functions, Deno Deploy, Netlify Edge, Fastly Compute | JavaScript or WebAssembly only | Quillo does not run there (no WebAssembly build: no `wasm` in `Cargo.toml`) | — |
| Windows (IIS FastCGI, Plesk on Windows) | as for cPanel | as `\etc\quillo\policy.toml` on the current drive, which any signed-in user can create at `C:\` (Q-1282) | yes |
| `quillo app` (desktop), phone apps | the developer | no policy is read at all (Q-1284) | — |

**What no change to Quillo can do:** hold a developer who runs their own binary, or who builds their own image. A policy binds the `quillo` that reads it, as the guide says. The provider's real lever there is the system: CageFS, `noexec`, LVE limits, the VM.

**My recommendation, in this order:**

1. **Fix the silent ones.**
   - Q-1283: a `quillo.policy` that is there but is not a readable file stops the program.
   - Q-1284: the desktop and phone starts read the policy as `quillo run` does.
   - Q-1282: Windows reads `%ProgramData%\Quillo\policy.toml`, which only administrators can write.
2. **Grow what a policy can say (question 1):**
   - a `[run]` section for the options of `quillo run` (Part D), each a forced value, a ceiling, or a switch the provider can turn off;
   - `[fs] paths`, where `read_file` and `write_file` may go;
   - `[env] set`, values the program gets whatever its `.env` says;
   - plugins pinned by hash.
3. **Let a provider put its layers where its platform allows (question 2).** `/etc/quillo/policy.toml` stays the anchor. It may name further files (`include`), and so may a folder `/etc/quillo/policy.d/`. The user's global path variable, `QUILLO_POLICY_PATH`, adds layers where the provider starts the process. A provider that builds its own `quillo` can compile in another anchor.
4. **Say what is in force (Q-1286):** `quillo policy main.qwe` prints the combined policy, each value with the file it came from. It exits non-zero when a layer that was asked for is missing, so a provider's health check can tell.
5. **A guide page, "Hosting Quillo for others"**, per platform: what the policy holds there, and what is the system's job (CageFS's `/etc/quillo`, `noexec`, the image).

## Part A: what was run

`provider.toml` (the provider's layer):

```toml
[plugins]
build = false
[effects]
allow = ["db", "http", "fs", "time", "random", "env"]
[http]
private = false
hosts = ["api.example.com"]
[env]
names = ["DATABASE_URL", "APP_NAME"]
[limits]
workers = 4
max_compute = "2s"
max_body = "1MB"
max_memory = "64MB"
```

The developer's `app/quillo.policy` asks for more (`hosts = ["*"]`, the `net` effect), and their `.env` sets `DATABASE_URL=from-dotenv`. Run as:

```sh
DATABASE_URL=from-provider quillo run app/main.qwe --policy provider.toml \
  --workers 64 --max-compute 60s --max-body 100MB --trust-proxy 0.0.0.0/0 --allow-path /etc
```

| What the developer did | Result | Held? |
|---|---|---|
| `--max-compute 60s` | a spinning request ended at 2.03 s, 500 | ✓ |
| `--max-body 100MB` | a 2 MB body: 413 | ✓ |
| `--workers 64` | `workers 4` in the start line | ✓ |
| `quillo.policy` with `hosts = ["*"]` and `net` | `example.com is not among the hosts provider.toml allows`; effects without `net` | ✓ |
| `http.get("http://169.254.169.254/...")` | refused by the hosts list | ✓ |
| `.env` with `DATABASE_URL` | the provider's value, `from-provider` | ✓ |
| `env("HOME")` | `unset` | ✓ |
| `--trust-proxy 0.0.0.0/0`, then a request with `X-Forwarded-For: 6.6.6.6` | `request.ip` = `6.6.6.6` | ✗ |
| `--allow-path /etc`, then `read_file("/etc/hosts")` | the file (without the option: `none`) | ✗ |
| `--with "sh -c 'echo ran-by-with: $(id -un)'"` | `[sh] ran-by-with: restiguay`, under a policy whose effects leave out `net` | ✗ (outside the policy) |
| `--describe` | `GET /_quillo/describe` → 200 | ✗ (outside the policy) |

What a provider gets if it tries to write such a setting into its file:

```
$ quillo check app/main.qwe --policy p.toml          # p.toml: [run] trust_proxy = ["127.0.0.1"]
quillo: p.toml:2: `run.trust_proxy` is not something a policy says (it says plugins.build, plugins.allow,
effects.allow, http.private, http.hosts, env.names, limits.workers, limits.max_compute, limits.max_body,
limits.max_tool_calls, limits.max_memory)
```

The same for `[limits] max_connections`, `[run] watch` and `[fs] paths`: exit 2. A provider cannot write down a default for anything outside the 11.

## Part B: how each layer is found (`src/policy.rs`, `load`)

| Layer | Where | When it is not there | When it is there but wrong |
|---|---|---|---|
| The host's | `/etc/quillo/policy.toml`, a constant (`HOST_FILE`). No variable, option or build setting names another | no policy, no word | stops the program (exit 2): unreadable, a broken link, or a line a policy does not say |
| `--policy FILE` | named on the command line | — | stops the program |
| The program's | `quillo.policy` beside the main file | no policy, no word | **a broken link or a folder: skipped, no word** (`program.is_file()`, Q-1283); unreadable: stops |

Which commands read them: `quillo run`, `quillo fcgi`, `quillo serve` and `quillo dap` (all through `run_program`), `quillo mcp`, `quillo check` and `quillo docs --mcp`. Not read: `quillo app` (`desktop.rs` calls `load` directly, past `with_policy`) and phone apps (`mobile.rs`, `start`). The start line names the files read, so an absent host file shows only as a line that does not mention it. `quillo check` names them only when `--policy` is given.

## Part C: the user's suggestion, a global path variable

`QUILLO_POLICY_PATH=/etc/quillo/policy.toml:/opt/host/plan-basic.toml`: a list of files, as `PATH` is, that every `quillo` reads before `--policy` and `quillo.policy`, each narrowing the last. Today the binary reads 23 `QUILLO_` variables (`QUILLO_SECRET`, `QUILLO_DB`, `QUILLO_JIT`, ...), and none names a policy.

**It solves where the file is, which is the larger part of Q-1281 and Q-1282.** A provider can put its policy where its platform lets it write: `/opt/...` in a provider-built image, a mounted secret in Kubernetes, `%ProgramData%` on Windows, a per-plan file per account.

**On its own it is not unoverridable: whoever starts the process sets its environment.**

| Platform | Who sets the variable | Unoverridable? |
|---|---|---|
| A systemd unit or a supervisor the host owns | the host | yes |
| A managed runtime whose start the provider owns | the provider. AWS Lambda works this way: by its documentation, its reserved variables cannot be set by the function (not run) | yes |
| cPanel: Apache's `FcgidInitialEnv` or `SetEnv` in the host's configuration | the host sets it, but the developer's own `app.fcgi` runs next and can `unset` it | no |
| Fly.io, Cloud Run, Render, Railway: the app's settings, the Dockerfile | the developer | no |
| Windows: a machine-wide variable | the administrator, but any process can change its own copy | no |

So the variable can add layers, but the anchor must be something the developer cannot switch off: a file at a place only the provider can write, which `quillo` always reads. Recommended, in order of reading:

1. **The anchor:** `/etc/quillo/policy.toml`, on Windows `%ProgramData%\Quillo\policy.toml`, or a path compiled into a provider's own build (`QUILLO_HOST_POLICY` at build time). It may say `include = ["/opt/plans/basic.toml"]` and `require_path = true`.
2. **`/etc/quillo/policy.d/*.toml`**, in name order: plans and drop-ins without editing one file.
3. **`QUILLO_POLICY_PATH`:** each file must exist, or the program stops. When the anchor says `require_path = true`, the variable itself must be set and name only files the anchor allows. A developer who unsets it then gets a program that does not start, not one without the provider's limits.
4. `--policy FILE`, then `quillo.policy`, as today.

Every layer only narrows, as today.

## Part D: what a `[run]` section would hold

Each option of `quillo run` that matters to a provider, with the kind of default it would be. A forced value cannot be changed. A ceiling may be lowered by the developer. A switch the provider turns off cannot be turned on.

| Option | Why a provider cares | `[run]` key | Kind |
|---|---|---|---|
| `--trust-proxy` | the provider's proxy is the only one whose `X-Forwarded-For` is true; any other makes `request.ip` forgeable (measured) | `trust_proxy = ["10.0.0.0/8"]` | forced |
| `--allow-path` | widens `read_file` and `write_file` past the program's folder (measured) | with `[fs] paths` | ceiling, or off |
| `--with` | runs a command beside the program (measured) | `with = false` | off |
| `--watch`, `--reload` | development behaviour in production: reloads, open pages refreshed | `watch = false` | off |
| `--describe` | publishes the program's API (measured: 200) | `describe = false` | off |
| `--instances` | cores the account may take | `instances = 2` | ceiling |
| `--max-connections`, `--connections-per-client`, `--live-per-client` | how many sockets one account holds | `max_connections = 200` ... | ceiling |
| `--listen`, `--host`, `--port`, `--socket-mode` | where the program may listen: a Unix socket only, on shared hosting | `listen = "unix"`, `socket_mode = "660"` | forced |
| `--base` | the path the provider mounts the site under | `base = "/shop"` | forced |
| `--plain-http` | a site that says it is served over plain HTTP on purpose | `plain_http = false` | off |
| `--split`, `--scale`, `--join`, `--register`, `--as-service` | processes and joins the account may make | `split = false`, `join = false` | off |
| `--drain` | how long a stop may wait | `drain = "30s"` | ceiling |
| `--no-check` | skipping the checks before running | `check = true` | forced |
| `--log` | the format the provider's collector reads | `log = "json"` | forced |
| values for `env()` | a default the program gets whatever its `.env` says (`APP_ENV`, a region) | `[env] set = { APP_ENV = "production" }` | forced |

## New issues

### Q-1280. A provider cannot hold a developer to the options of `quillo run` outside the policy's 11 settings, nor even write them down

| Kind | Detected by | Attempts |
|---|---|---|
| limitation; security | `quillo run` with curl; `quillo check` | 1 each |

Measured in Part A: under a provider's policy, `--trust-proxy 0.0.0.0/0` made `request.ip` whatever the visitor's `X-Forwarded-For` said (`6.6.6.6`), and `--allow-path /etc` let `read_file("/etc/hosts")` return the file, which is refused without it. `--with` ran a shell command, and `--describe` published the API. A policy that tries to say any of these stops with "is not something a policy says".

| Impact | |
|---|---|
| **Functionality** | Of the 40-odd options of `quillo run`, a provider can bound 4 (workers, max-compute, max-body, max-memory) and `--max-tool-calls`. The rest are the developer's |
| **User** | A provider cannot offer a plan ("2 instances, a Unix socket, no `--with`, JSON logs") that Quillo itself keeps. It has to trust each account's `app.fcgi` |
| **Requests** | Rate limits, logs and abuse reports go by a `request.ip` the visitor can choose, whenever an account trusts every proxy |
| **Security** | A forged `request.ip` steps around limits by address. A widened `read_file` reaches what the account's Unix user can read. Both are the account's own reach, but they are past what the provider's policy meant to allow |

**Suggestion:** the `[run]` section of Part D, `[fs] paths`, and `[env] set`, each only narrowing, as the rest of the policy does (question 1).

### Q-1281. The provider's file has one fixed place, and an absent one is no policy, without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent; security | reading `src/policy.rs`; CageFS and the container platforms from their documentation (not run) | 1 |

`HOST_FILE` is the constant `/etc/quillo/policy.toml`. No variable, option or build setting names another place, and `load` treats an absent file as no policy. Where `/etc` is not the provider's, or not what the program sees, the provider's limits silently do not apply:

- **CloudLinux's CageFS**, which most cPanel hosts run. Each account sees its own `/etc`, made from a skeleton. A folder the host adds to the real `/etc` is not in it unless the host also mounts it into CageFS.
- **Containers and microVMs** (Fly.io, Cloud Run, Lambda): the image is the developer's. A provider that builds a base image cannot keep the developer from replacing `/etc/quillo`.
- **Several plans on one machine:** one file for every account. Per-account limits need per-account files, which only CageFS's per-account `/etc` could give, and only by hand.

| Impact | |
|---|---|
| **Functionality** | A provider that wrote a policy may be running every program without it |
| **User** | Nothing says so: the start line names the files read and never says one was missing |
| **Requests** | No change |
| **Security** | The limits the provider meant (plugins, hosts, private addresses, environment names) are not in force |

**Suggestion:** Part C. The anchor may name further files and require them; `/etc/quillo/policy.d/`; `QUILLO_POLICY_PATH`; a compiled-in anchor for providers' own builds. The guide's shared-hosting page says to add `/etc/quillo` to CageFS (`/etc/cagefs/cagefs.mp`, then a remount). `quillo policy` (Q-1286) lets the host check from inside an account what is in force.

### Q-1282. On Windows, the provider's file is `\etc\quillo\policy.toml` on whichever drive the program runs from

| Kind | Detected by | Attempts |
|---|---|---|
| security | reading the code; Windows' rules for paths and its default rights on `C:\` (not run) | 1 |

The release has a Windows build (`quillo-v0.8.9-x86_64-pc-windows-msvc.zip`). There, the constant path `/etc/quillo/policy.toml` has a root but no drive, so it is read from the root of the current drive: `C:\etc\quillo\policy.toml`, or `D:\etc\...` for a program started from `D:`. By Windows' default rights on a system drive's root, any signed-in user may create a folder there.

| Impact | |
|---|---|
| **Functionality** | No place an administrator owns. The file read changes with the working folder's drive |
| **User** | A provider on Windows (IIS FastCGI, Plesk) has no documented place for its policy |
| **Requests** | No change |
| **Security** | Any user can create `C:\etc\quillo\policy.toml`. A policy can only narrow, so they gain nothing for themselves, but an invalid file stops every Quillo program on that drive (exit 2). The administrator's own policy has no protected home |

**Suggestion:** on Windows, the anchor is `%ProgramData%\Quillo\policy.toml`, which only administrators may write by default, found through the system's known folder, never the current drive. Under `cfg(windows)`, the old path is not read.

### Q-1283. A `quillo.policy` that is a broken link or a folder is skipped without a word, and the program runs with none

| Kind | Detected by | Attempts |
|---|---|---|
| silent; security | `quillo check`, `quillo run`, curl | 1 |

`load` reads the program's file only when `program.is_file()`. A deploy that keeps `quillo.policy` in a shared folder and links it into each release (`ln -s ../shared/quillo.policy`) loses it as soon as the link points nowhere:

```
linkapp/quillo.policy -> ../shared/quillo.policy     (missing)
linkapp/main.qwe:  route GET / = str(http.get("https://example.com/").error ?? "reached example.com")
```

`quillo check linkapp/main.qwe` says `ok`, `quillo run` starts with no policy line, and `GET /` answers `reached example.com`. With the intended file (`hosts = ["api.example.com"]`) it is refused. A folder named `quillo.policy` does the same. The host's file, by contrast, stops the program when it is a broken link (`symlink_metadata`, then the read fails).

| Impact | |
|---|---|
| **Functionality** | The site owner's limits are gone after a deploy that broke a link |
| **User** | Nothing says so, in `check` or in `run` |
| **Requests** | No change |
| **Security** | Every host, every effect, every variable the host allows, instead of what the site owner narrowed it to |

**Suggestion:** treat the program's file as the host's: absent (no entry at all) is no policy, while anything else at that name must be a readable file or the program stops, naming it (`quillo.policy is a link to ../shared/quillo.policy, which is not there`).

### Q-1284. The desktop and phone starts read no policy, and `quillo app --policy FILE` is taken and ignored

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading `src/desktop.rs`, `src/mobile.rs`, `src/main.rs` (the release has no `app`; not run) | 1 |

`quillo app` parses `run`'s options, `--policy` among them, then calls `load` directly. Only `run_program`, `mcp`, `check` and `docs` call `with_policy`, so neither the host's file, the `--policy` given, nor `quillo.policy` is read. A phone app's `mobile::start` makes its `Interp` without one either. The release's `quillo` is built without windows, so this touches custom builds and phone apps.

| Impact | |
|---|---|
| **Functionality** | A program's own narrowing (its hosts, its effects) does not follow it into its desktop or phone app |
| **User** | `--policy` given to `quillo app` does nothing, and says nothing |
| **Requests** | No change |
| **Security** | A program that narrows `http` to its own API in `quillo.policy` reaches anything in its app build |

**Suggestion:** `desktop::window` and `mobile::start` read the policy as `run_program` does (the host's file only where there is one: none on a phone), or `quillo app` refuses `--policy` and the guide says the apps run without one.

### Q-1285. `[plugins] allow` names files, so a plugin renamed to an allowed name passes, although the catalog knows each plugin's hash

| Kind | Detected by | Attempts |
|---|---|---|
| limitation; security | the guide ("It names plugins; it does not prove their code"); `src/install.rs` | 0 (not run: it needs a plugin built) |

A provider that allows `"qr"` allows any file named `qr.qwer`, or its prebuilt library in `.quillo/native`, whoever wrote it. `quillo install` already checks each catalog plugin's version and hash.

| Impact | |
|---|---|
| **Functionality** | The list cannot say "the catalog's `qr`, this exact one" |
| **User** | A provider cannot offer "catalog plugins only" with Quillo's help |
| **Requests** | No change |
| **Security** | Any Rust a developer renames runs in the server's process, under a policy that meant to allow one known plugin |

**Suggestion:** `allow = ["qr@sha256:…"]` holds a plugin to its hash: the built library's SHA-256, which `.quillo/plugins.lock` already records for each library, and the source's for a plugin built on the server. `[plugins] catalog = true` takes only what `quillo install` checked. The guide's "Not built yet" line names signed catalogs. A hash needs no signature and is there today.

### Q-1286. Nothing shows the combined policy and where each value came from

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | `quillo check`, `quillo run` | 1 |

`quillo run` prints one line naming the files read and the limits. `quillo check` prints nothing about the policy unless `--policy` is given (`okapp`, with a `quillo.policy`, says only `ok`). There is no command that says what is in force, which layer set it, or that a layer was missing.

| Impact | |
|---|---|
| **Functionality** | No change |
| **User** | A developer on a host learns its limits when a request is refused. A provider cannot check from inside an account that its file is seen (Q-1281) |
| **Requests** | No change |
| **Security** | A missing layer goes unnoticed |

**Suggestion:** `quillo policy [main.qwe]` prints each setting with the file it came from (`http.hosts: api.example.com (provider.toml)`), and exits 1 when a required layer is missing, so it can be a host's health check.

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| Planned to test the host's file in a Linux container | the usual way | no Docker on this machine, and `/etc` needs root | the provider's layer played by `--policy`, the rest read in the code |
| Took `quillo dap` for a start without a policy | it is another command | it goes through `run`, which reads it; corrected | 1 read |

## Limitations

- **A developer's own binary or image is outside any policy.** That is the system's to stop: CageFS, `noexec` home folders, LVE, the VM.
- **Isolate edges cannot run Quillo** (JavaScript or WebAssembly only; Quillo is a native program with threads, sockets and SQLite).
- **A plugin's Rust is outside the policy once loaded**, as the guide says: the policy chooses which plugins load.
- **`[http] private` covers `http.*`, `site` calls and push**, not `db.connect` or UDP, as the guide says.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| The developer's `quillo.policy` asking for more hosts and effects | narrowed to the provider's, no error |
| `--max-compute`, `--max-body`, `--workers` above the provider's | held to the provider's (2.03 s 500; 413; workers 4) |
| `.env` setting a variable the provider's environment sets | the provider's value |
| A variable outside `[env] names` | not set |
| The cloud's metadata address | refused |
| Push to an address the policy does not allow (read in the code) | refused as it is sent |
| A host file that is a broken link (read in the code) | the program stops |
| `quillo serve DIR` (read in the code and the guide) | reads the host's file and `DIR/quillo.policy`, and serves neither |
| Push to a private address (read in the code) | refused: `policy.http` resolves the name and checks private addresses as well as hosts |

## Questions for the user

### 1. Should a policy be able to set the options of `quillo run`, the file paths and environment values, as defaults the developer can only narrow?

**Yes. It is what "unoverridable defaults" asks for, and today a provider cannot write any of them down (Q-1280).** It happens on every host that runs others' programs. Measured: `--trust-proxy 0.0.0.0/0` forges `request.ip`, `--allow-path /etc` reads `/etc/hosts`, and a policy naming either stops with exit 2.

| Impact | |
|---|---|
| **Functionality** | `[run]` (the keys of Part D), `[fs] paths`, `[env] set`. Each is a forced value, a ceiling or an off switch, and later layers only narrow |
| **User** | A provider can offer plans that Quillo keeps. A developer sees what they are held to (`quillo policy`) |
| **Requests** | Correct `request.ip` behind the provider's proxy; connection and instance ceilings per account |
| **Security** | The provider's proxy, paths and processes are the provider's, not each `app.fcgi`'s |

What today's way has that this would lose: a policy small enough to read at a glance, 11 settings.

**My recommendation:** build it, starting with `trust_proxy`, `[fs] paths`, `with`, `watch` and the connection ceilings, the ones measured or most asked for by shared hosts.

**The question:** shall I plan `[run]`, `[fs] paths` and `[env] set` as in Part D? If not, the guide says which options a host must set in its own wrapper, and that a developer's `app.fcgi` can change them.

### 2. Should a provider's layers be found through a global path variable as well as `/etc`?

**Yes, as the user suggested, but as an addition to an anchor the developer cannot switch off. Whoever starts the process sets its environment (Part C).** It happens on every platform where `/etc` is not the provider's: CageFS, containers, Windows, several plans on one machine.

| Impact | |
|---|---|
| **Functionality** | `QUILLO_POLICY_PATH`, read after the anchor and `policy.d/`, before `--policy`. Each file must be there. The anchor may name files (`include`) and require the variable (`require_path = true`) |
| **User** | A provider puts its policy where its platform lets it write: a provider-built image, a mounted secret, `%ProgramData%`, a file per plan |
| **Requests** | No change |
| **Security** | Where the provider starts the process, its layers cannot be dropped. Where the developer does, the anchor's `require_path` stops a program whose variable was unset, rather than running it without the limits |

What today's way has: one file in one place, nothing to look up.

**My recommendation:** all four ways in, each with its role: the anchor (`/etc`, `%ProgramData%`, or compiled in) as the one a developer cannot switch off; `policy.d/` for plans; the variable for platforms whose start the provider owns; `include` and `require_path` to tie them together.

**The question:** shall I plan `QUILLO_POLICY_PATH` with the anchor's `include` and `require_path`? If not, only `policy.d/` and the Windows anchor, and the guide says how to make `/etc/quillo` visible under CageFS.

### 3. Should a layer that is there but broken always stop the program?

**Yes. The host's file already does. The program's own does not (Q-1283), and a new variable's files should not be skipped either.** It happens after a deploy that breaks a link, or a mount that did not come up.

| Impact | |
|---|---|
| **Functionality** | A `quillo.policy`, or a file a `QUILLO_POLICY_PATH` names, that is a broken link, a folder or unreadable: the program stops (exit 2), naming it |
| **User** | A failed start with the reason, instead of a site running with fewer limits than meant |
| **Requests** | Until it is fixed, the site is down rather than open |
| **Security** | The limits written are the limits in force, or nothing runs |

What today's way has: a site that keeps running after a deploy slip.

**My recommendation:** stop, as the host's file does.

**The question:** shall a broken `quillo.policy` stop the program? If not, it warns at start and in `quillo check`, naming the file.
