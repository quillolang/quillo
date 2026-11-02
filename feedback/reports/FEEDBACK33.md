# Feedback from pointing a phone app at a domain

| | |
|---|---|
| App | A probe app ("Endpoints", `org.quillolang.endpointprobe`), built to answer one question: how can a Quillo program running in a phone app reach its server at a domain (`https://app.example.com`) without much configuration? It has an `app.toml`, a `.env` with an address and a fake key, and a `.git/HEAD`. It also had two programs under `quillo run`: a "domain" with `server fn`s and a session, and a "caller" standing in for the phone's program |
| Quillo | 0.6.0 (378e64b, 2026-11-01), branch `feedback29-fixes`, `./target/release/quillo`; the iOS runtime rebuilt from the same tree for `aarch64-apple-ios-sim` by `mobile/ios/build.sh` |
| Model | Claude (Opus 5.5) |
| Tested with | the iOS 27 simulator (iPhone 18 Pro), driven by `--eval`, with screenshots; `./gradlew :app:mergeDebugAssets` on a `.env`, a `.git/` and a `_lib/` folder; curl; reading [src/mobile.rs](../../src/mobile.rs), [mobile/ios/Quillo/App.swift](../../mobile/ios/Quillo/App.swift), [MainActivity.kt](../../mobile/android/app/src/main/java/org/quillolang/app/MainActivity.kt), [Bridge.kt](../../mobile/android/app/src/main/java/org/quillolang/app/Bridge.kt), both `build.sh`, [src/service.rs](../../src/service.rs), [src/fetch.rs](../../src/fetch.rs), [src/server.rs](../../src/server.rs), [src/dotenv.rs](../../src/dotenv.rs), mobile/README.md and the site's guides/phone-apps.md |

What worked well: the program inside the app can reach any domain today. A `server fn` that calls `http.get("https://quillolang.org/")` answered `status 200` in the simulator, and Fieldbook's weather call did the same on a real iPhone (FEEDBACK18). TLS works with no setup. A deployed Quillo site's `server fn`s can be called from another program, the phone's included, at `POST /_quillo/rpc/<fn>` with a JSON array of arguments; the answer is `{"ok": ...}`.

So the answer is that it works, but only with the address written into the code, and only from the program's side, not the page's. Everything that would make it one line of configuration is missing or blocked:

| Way to reach `https://app.example.com` | Today | What it takes | Issue |
|---|---|---|---|
| The program's `http.*` in a `server fn`, the address in the code | works | an edit to the code for each address (staging, production) | |
| The same, the address from `env("API_URL")` and a `.env` | `env()` gives its default on the phone, though the `.env` is in the app | nothing works | Q-406 |
| A deployed Quillo site's `server fn`s (`/_quillo/rpc/<fn>`) | works, but the site's session is lost between calls | an undocumented wire, cookies by hand | Q-412 |
| `service shop = "shop.qwe"`, sent to the domain | not possible | `--service-url` and `QUILLO_SERVICE_SHOP` cannot be given to an app, and a call needs a secret an app cannot keep | Q-413 |
| The page's own `http.*` | refused: `http: Load failed` | `route OPTIONS` and CORS headers on the domain, for an origin that is not stable | Q-411 |
| The web view on the domain (the app as a shell around the live site) | a link or `redirect` to it opens Safari; `window.quilloNative` answers 127.0.0.1 only | not possible | Q-410 |
| An `<iframe>` of the domain in a page | the person is sent to Safari as the page loads | | Q-409 |

On the way, the iOS app was found to carry the program folder's `.env` and `.git/`. Android silently leaves out dotfiles and also folders starting with `_`.

Fix Q-407 first: every iOS build, and any upload to App Store Connect, ships the secrets a server program keeps in its `.env`. Q-409 is next, a one-line fix: a page with an embedded video or map throws the person out of the app. Then Q-406 is the one line this report was asked about; "What would make it one line" below says how.

The repros are inline, each run on 378e64b. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched; the index still has no rows for FEEDBACK30 to FEEDBACK32 (Q-361 to Q-405), so these rows follow theirs. Numbers start at Q-406.

## Seen again

| ID | What it was | Here |
|---|---|---|
| Q-124 | `quillo app` read no `.env` | fixed for `quillo app` by sharing `run`'s start-up (`before_the_program` in src/main.rs); the third launcher, `mobile::start`, never took it: Q-406 |
| Q-339 | a phone app got a new origin at each launch | fixed by keeping the port; the origin is still `http://127.0.0.1:<port>`, which matters for CORS (Q-411) |
| Q-346 | the phone apps dropped links | fixed by sending other sites to the system; the same rule now catches iframes (Q-409) and leaves no way to stay on the domain (Q-410) |

## New issues

### Q-406. A phone app has nowhere to put an endpoint's address: the bundled `.env` is not read, so `env("API_URL")` gives its default on the phone

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone only | the probe under `quillo run` and in the simulator | 1 |

The address of a server differs between a developer's machine, staging and production, so it belongs in configuration. Quillo's configuration is the environment and the `.env` beside the program, which `quillo run` and `quillo app` read (`before_the_program`, src/main.rs). [`mobile::start`](../../src/mobile.rs) does not call it, and nothing else sets the program's environment: a phone app has no command line, and iOS and Android give the app's process no variables the developer chooses. The `.env` is copied into the app (Q-407) and into the app's data folder at launch, and then not read.

```sh
# .env:     API_URL=https://quillolang.org
# main.qwe: <p>env("API_URL"): {env("API_URL", "not set")}</p>
quillo run main.qwe          # env("API_URL"): https://quillolang.org
mobile/ios/build.sh . --run  # env("API_URL"): not set
```

Today the address has to be a constant in the code, chosen per build by editing it, or by `main = "mobile.qwe"` in `app.toml` with the constant in `mobile.qwe` (Q-169). The phone-apps guide and mobile/README.md say nothing of `.env` or `env()` on a phone, or of reaching a server at all.

**Suggestion:** `app.toml` already goes into the app (it was in `Quillo.app/program/` beside `main.qwe`), and whatever is in it is public by nature. Take public values from it, `env.API_URL = "https://app.example.com"` (a dotted key: valid TOML, and one key per line as app-config.sh reads it). In `mobile::start`, read the `env.*` lines of `<app>/app.toml` into `interp.env_file`, as a service's own `.env` is read ([src/service.rs:1185](../../src/service.rs)), not with `set_var`: the launcher's other threads are already running. `env("API_URL")` then gives the value on the phone, while `.env` gives it under `quillo run`. Say so in the phone-apps guide, beside `main`.

### Q-407. The iOS app carries the program's `.env` and `.git/`, so a server program's secrets ship in every build

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | listing the built app | 1 |

`mobile/ios/build.sh` copies every file of the program's folder except databases, `uploads/` and `.quillo/` ([line 58](../../mobile/ios/build.sh)). A program that runs on a server keeps its secrets in `.env`: `QUILLO_SECRET`, `QUILLO_SERVICE_SECRET`, `DATABASE_URL`, API keys. The guide teaches this. When the same folder is built for the phone, as `main = "mobile.qwe"` invites, the secrets go into the app bundle and into an `.ipa`, where unzipping it reads them. They are also copied into the app's data folder at each launch, where they do nothing (Q-406). A `.git/` goes in too: the program's whole history, past secrets included.

```sh
mobile/ios/build.sh scratch/endpoints --run
find mobile/ios/build/iphonesimulator/Quillo.app/program -type f
# .../program/.env
# .../program/main.qwe
# .../program/app.toml
# .../program/.git/HEAD
cat mobile/ios/build/iphonesimulator/Quillo.app/program/.env
# API_URL=https://quillolang.org
# SECRET_KEY=sk_live_not_a_real_key_but_it_ships
```

Android's build.sh has the same filter, but Gradle drops the files itself (Q-408), so its APK does not have them. A `.env.production` or `secrets.json` would go into both.

**Suggestion:** decide what goes into an app in one place, app-config.sh, for both scripts. Leave out `.env` and `.env.*`, `.git/`, `.quillo/` and other dotfiles, and print what was left out, as the script prints the programs it skips. Refuse a `.env` outright with a message ("a .env is the server's; put public values in app.toml as env.NAME"), so nobody counts on it reaching the phone. Add a test that builds a bundle from a folder with a `.env` and a `.git/` and finds neither.

### Q-408. Android leaves out folders that start with `_` and every dotfile, so a program can work on iOS and not on Android

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone only | `./gradlew :app:mergeDebugAssets` with a `.env`, a `.git/HEAD` and `_lib/x.qwe` in the program's assets | 1 |

Gradle filters assets with aapt's default pattern, `!.svn:!.git:!.ds_store:!*.scc:.*:<dir>_*:!CVS:!thumbs.db:!picasa.ini:!*~`. It drops every name starting with `.` and every folder starting with `_`, without a word. The merged assets had `main.qwe` and none of the three. On Android this hides Q-407 by luck. But a program that keeps code in `_lib/` or `_partials/` and takes it with `import _lib/x.qwe use x` builds and runs on iOS, then fails at launch on Android with the file missing. A `.well-known/` folder under `public/` is lost the same way.

**Suggestion:** set `androidResources { ignoreAssetsPattern = "!.svn:!.git:!.ds_store" }` (or narrower) in app/build.gradle.kts, so Gradle keeps what build.sh put there, and let app-config.sh's one filter (Q-407) decide.

### Q-409. On iOS an `<iframe>` of another site sends the person to Safari as the page loads

| Kind | Detected by | Attempts |
|---|---|---|
| silent, phone only | the probe's `/frame` in the simulator, with a screenshot | 1 |

[App.swift's `decidePolicyFor`](../../mobile/ios/Quillo/App.swift) (line 186) sends every navigation that is not to the program's origin to `UIApplication.shared.open` and cancels it. WebKit asks the same question for a frame's load, and the check does not look at `action.targetFrame?.isMainFrame`. A page with `<iframe src="https://app.example.com/">`, a YouTube embed or an OpenStreetMap embed opens Safari on that address as soon as it is shown. The app's page loses the frame, and the person finds themselves in Safari with "◀ Endpoints" in the corner.

```quillo
route GET /frame = <html><body><h2>An iframe of the domain</h2><iframe src="https://quillolang.org/" width="320" height="400"></iframe></body></html>
```

```sh
xcrun simctl launch --terminate-running-process booted org.quillolang.endpointprobe --eval "location.href='/frame'"
# 6 s later: Safari in front, on quillolang.org
```

Android was not run. [MainActivity.kt's `shouldOverrideUrlLoading`](../../mobile/android/app/src/main/java/org/quillolang/app/MainActivity.kt) (line 92) does not look at `req.isForMainFrame` either.

**Suggestion:** let frames load where they are, as a browser does: `if action.targetFrame?.isMainFrame == false { return decisionHandler(.allow) }`, and on Android `if (!req.isForMainFrame) return false`. A frame from another origin gets no `window.quilloNative` either way, since both bridges check the frame's origin. Add a `--eval` step to the iOS checks: a page with an iframe stays the app's.

### Q-410. The app cannot be the site: there is no setting to open the domain in the web view, and the bridge answers only 127.0.0.1

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading App.swift, MainActivity.kt and both bridges; the link on the probe's page | 1 |

A program already live on `https://app.example.com` has one way into a phone app with no new code: a web view on the site, with the phone's hardware added. Quillo's apps cannot do it. Both launchers always start the bundled program and open `http://127.0.0.1:<port>`. A link or a `redirect` to the domain leaves for Safari or Chrome (as Q-346 decided for other sites). `window.quilloNative` refuses any frame whose host is not `127.0.0.1` (App.swift line 257; Bridge.kt `attach(origin)`). The cleartext and transport settings (`NSAllowsLocalNetworking`, network_security_config.xml) are written for 127.0.0.1 only, which is right for https but leaves no room for a staging server on plain http.

Such an app gives up what the phone build is for: it does not work offline and keeps no data on the phone. Apple's review guideline 4.2 turns down apps that are a repackaged website. So this is not the main way, but it is the least configuration there could be, and a common first step.

**Suggestion:** an `app.toml` key, `site = "https://app.example.com"`, read by the build into Info.plist and strings.xml. Pages of that origin stay in the web view, and `window.quilloNative` answers them as it answers the program's pages. The bundled program still starts, and its first page can link or redirect to the site, so the app can show a page of its own when the phone is offline. Only https; one origin, written by the developer, never taken from a page.

### Q-411. A page cannot call the domain: Quillo answers no CORS, the page's origin is not one a site can safely allow, and the error says only "Load failed"

| Kind | Detected by | Attempts |
|---|---|---|
| loud, docs | the probe's component in the simulator | 1 |

`http.get` in a handler or `@mount` runs in the web view (`B.http.get`, a `fetch`). From the app's page, at `http://127.0.0.1:<port>`, the domain is another origin. The guide says a Quillo server answers no preflight and writes no `access-control-*` header unless the program declares `route OPTIONS` and sets them (USERGUIDE.md line 1857). So the call fails, and the error value reads only `http: Load failed`. It does not name CORS, the other site or plain http (WebKit's TypeError says no more). A `server fn` on the domain refuses the call anyway: "server fns can only be called from the program's own pages" (src/server.rs `cross_site`), which is right.

The domain could allow the app's origin, but `http://127.0.0.1:<port>` changes when the kept port is taken (Q-339). Any program on any visitor's computer can serve on that origin, a `quillo run` included. Allowing it with credentials therefore allows every local server.

```quillo
<section @mount={() => {
    let r = http.get("https://quillolang.org/")
    page = is_err(r) ? `error: ${r.error}` : `status ${r.status}`
}}>
```

```
page's http.get: error: http: Load failed
```

**Suggestion:** in the browser runtime, when `fetch` throws a TypeError for another origin, say so: "http: no answer from quillolang.org: it may not allow this page (CORS), or be plain http, or be unreachable". The phone-apps guide should tell a program to reach its server from a `server fn`, and why.

### Q-412. Calling a deployed Quillo site from a phone's program works only through an undocumented wire, and the site's session is lost

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | two programs under `quillo run` on 127.0.0.1 | 1 |

The natural shape for a Quillo phone app with a server is the same program on both sides: the phone calls the site's `server fn`s. The only way is the wire the pages use, which the guide does not document for programs. The caller has to know the path, send a JSON array, and unwrap `{"ok": ...}` or `{"err": ...}` itself. `http.*` keeps no cookies, so a session the site starts is gone by the next call:

```quillo
// the domain
server fn notes() = [{ id: 1, text: "from the domain" }]
server fn sign_in(name: str) { set_session({ user: name }); name }
server fn who() = session()?.user ?? "nobody"
route GET /api/notes = respond(200, json.stringify(notes()), { "content-type": "application/json" })
```

```quillo
// the phone's program
let DOMAIN = "http://127.0.0.1:4520"
fn call(name: str, args: list) with http {
    let r = http.post(`${DOMAIN}/_quillo/rpc/${name}`, args, { "content-type": "application/json" })
    is_err(r) ? r : r.json
}
server fn try_all() with http {
    { rpc_notes: call("notes", []), route_notes: http.get(`${DOMAIN}/api/notes`).json, signed_in: call("sign_in", ["ann"]), then_who: call("who", []) }
}
```

```
{"rpc_notes":{"ok":[{"id":1,"text":"from the domain"}]},"route_notes":[{"id":1,"text":"from the domain"}],
 "signed_in":{"ok":"ann"},"then_who":{"ok":"nobody"}}
```

The cookie the site sets is `__Host-quillo_session=...; HttpOnly; Secure; SameSite=Lax`. The caller would have to take it from `set-cookie`, keep it in its own database, and send it with each call by hand.

**Suggestion (a design for the user to decide):** a declaration that mirrors `service`, for a site rather than a service:

```quillo
site shop = env("SHOP_URL", "https://shop.example.com")
let mine = shop.notes()          // POST <url>/_quillo/rpc/notes; the value, or the site's error value
```

Its cookies would be kept per site in the app's data folder, so a person signed in on the phone stays signed in with the site. `quillo check` could check the arguments against the site's `quillo docs` document when it is at hand. Until then, document `/_quillo/rpc/<fn>` (JSON array in, `ok`/`err` out) as a stable wire for programs, and give `http.*` an opt-in cookie jar (`{ cookies: "shop" }`).

### Q-413. A service cannot be sent to a domain from a phone

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, docs | reading src/service.rs and mobile/README.md | 1 |

src/service.rs opens with "Where a service runs is configuration, not code": `service shop = "shop.qwe"` runs here, or wherever `--service-url shop=...` or `QUILLO_SERVICE_SHOP` points. On a phone, neither can be given (Q-406), so the bundled copy always runs, against the phone's SQLite. Even with a way to give the address, a call elsewhere is signed with `QUILLO_SERVICE_SECRET` ([src/service.rs:223](../../src/service.rs)). In an app, that secret is public (Q-407), and it signs calls to every service that shares it. That is right: services are for servers. But the docs do not say that the "configuration, not code" promise stops at the phone. A reader of the microservices guide will try it.

**Suggestion:** one sentence in the phone-apps guide's "What a phone build leaves out": a service runs inside the app; to use a server's, call the server (Q-412). `quillo check` on a phone build's `main` could warn on a `service` whose file holds a `db.open` of a server database.

### Q-414. Smaller gaps: plain http from the program, the program's certificate roots, the local network, and the guide

| Kind | Detected by | Attempts |
|---|---|---|
| silent, docs | the probe in the simulator; reading Cargo.lock and Info.plist | 1 |

- **Plain http.** App Transport Security and Android's network_security_config.xml cover the web view and the system's HTTP stacks, not the runtime's own sockets. In the simulator, the program's `http.get("http://example.com/")` answered `status 200`. The page's `fetch("http://example.com/", { mode: "no-cors" })` failed with `TypeError: Load failed`, while the same to https answered. A program can therefore send a password over plain http to a staging domain while the app's settings say it cannot. `quillo check` could warn on a non-local `http://` address in a phone build's code.
- **Certificate roots.** `http.*` uses ureq 2.12.1 with rustls and webpki-roots 0.26, the roots compiled into the binary, not the phone's. A domain whose certificate comes from a company CA installed on the phone (a managed device), or from a developer's own CA such as mkcert, opens in Safari and fails from the program. Read, not run: it needs a CA installed in the simulator's keychain.
- **The developer's own machine.** The simulator shares the Mac's network, so `http://127.0.0.1:8080` reaches a `quillo run` there. A real iPhone needs the Mac's LAN address, and iOS asks the person before an app reaches the local network. Info.plist has no `NSLocalNetworkUsageDescription` to say why. Not run on a phone.
- **The guide.** The phone-apps guide (site and mobile/README.md) says the program "is the same one you run as a server", but not how the app talks to a server: not `http.*` from a `server fn`, not `env()` on a phone, and not that pages cannot call the domain.

## What would make it one line

Q-406's fix and Q-407's filter together give this:

```toml
# app.toml, which goes into the app and so holds nothing secret
env.API_URL = "https://app.example.com"
```

```quillo
server fn notes() with http, env {
    let r = http.get(`${env("API_URL")}/api/notes`)
    is_err(r) ? r : r.json
}
```

Under `quillo run` the same code takes `API_URL` from `.env`, which never reaches the app. Q-412's `site` declaration would make it shorter still, and keep the person signed in. Q-410's `site =` key is the other end: no program code at all, at the cost of offline use.

## Where the AI struggled

| What I did first | Why | What happened | Cost |
|---|---|---|---|
| `const DOMAIN = "..."` at the top of a file | JavaScript's habit | `expected end of statement, found DOMAIN`; Quillo has `let` | 1 run |
| `server fn from_program() with http` around `env(...)` | `env` read as a plain function | `quillo check`: "`env` needs the env effect"; `with http, env` | 1 check |
| Expected Android to ship the `.env` too | the two build.sh files have the same `find` | Gradle's own asset filter dropped it, and `_lib/` with it (Q-408) | 1 Gradle run |

## Limitations

- Not run on a real phone or on the Android emulator. Android was checked as far as Gradle's merged assets; Q-409's Android side and Q-414's local-network and certificate points are from reading the code.
- `http://127.0.0.1:4520` stood in for the domain in Q-412; the rpc path, the JSON shapes and the cookie are the same on a deployed site served over https.
- The probe app was uninstalled from the simulator afterwards. `mobile/ios/build/` (git-ignored) holds its build, and the iOS simulator runtime in `mobile/quillo-mobile/target/` was rebuilt at 378e64b.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| The program's `http.get` over https from the simulator | `status 200`, no setup; the TLS roots are built in |
| The program's call to a site's `server fn` with no `Origin` (as `http.*` sends) | answered: `{"ok":[...]}` |
| The same call with another site's `Origin` | 403 "server fns can only be called from the program's own pages" |
| `OPTIONS` on `/_quillo/rpc/notes` | 405, as the guide says (no preflight answered by itself) |
| `platform().runtime` in the app | `mobile` |
| A `.git/` or `.env` on Android | left out of the merged assets (by luck: Q-408) |
| `window.quilloNative` in a frame of another origin | refused on both systems, by the frame's origin (read) |
