# Feedback from a study of running Quillo with no HTTP server: in the app itself, on iOS, Android, HarmonyOS, macOS, Windows and Linux

| | |
|---|---|
| App | None. The question: today a phone app and `quillo app` start the program's HTTP server on 127.0.0.1 and point a web view at it. Can the program run inside the app with no socket at all, on every system Quillo targets? Part 1 lists what the socket costs now. Part 2 measures what a web view does when the page comes from the app itself through a custom scheme (on macOS's WKWebView, the engine iOS uses), and reads what the other systems' web views offer. Part 3 is a design, and Part 4 the order to build it in |
| Quillo | 0.6.0 (23146e4, 2026-11-02). The binary is `feedback27-35-fixes`'s release build, built 12:14 today with that branch's uncommitted changes and copied to a scratch folder. It is built without the `app` feature, so `quillo app` was read, not run. Line numbers are the branch's working tree |
| Model | Claude (Opus 5.5) |
| Tested with | A 230-line Rust program on wry 0.57.0 and tao 0.37.1 (the versions `quillo app` uses): one window whose page is served by a custom scheme `q://`, run four times, and the same page's timing against `quillo run` on 127.0.0.1. wry was then copied and patched to say which body WebKit handed over. macOS 26.7.1 (25G241), Apple silicon. Read: [src/mobile.rs](../../src/mobile.rs), [src/desktop.rs](../../src/desktop.rs), [src/httpd.rs](../../src/httpd.rs), [src/server.rs](../../src/server.rs) (`Worker::answer`, `launch_gate`, `serve_live`, `serve_channels`, `waiter`), [src/jobs.rs](../../src/jobs.rs), [src/client/runtime.js](../../src/client/runtime.js) (`rpc`, `liveLoop`, `chanSocket`), [mobile/README.md](../../mobile/README.md), [mobile/harmony/PLAN.md](../../mobile/harmony/PLAN.md), and wry's handlers for each system |

No Quillo source was changed. Neither `feedback/repros/` nor `feedback/INDEX.md` was touched, as other sessions are working in the checkout. Numbers start at Q-465. The probe is below.

What worked well: Quillo is closer to this than it looks. A request and an answer are plain structs (`HttpRequest`, `HttpResponse` in [src/server.rs:12](../../src/server.rs)), and routing, sessions, `server fn` calls and pages are made from them, not from the socket. Live data already falls back from a WebSocket to a long poll by `fetch` ([runtime.js:4194](../../src/client/runtime.js)), and a long poll works over a custom scheme. The `server fn` wire (`/_quillo/rpc/<fn>`, JSON in and out) is stable and documented. A page from a custom scheme is a secure context on WebKit (`isSecureContext` true, `crypto.subtle` and `getUserMedia` there), its origin never changes, and its `localStorage` was still there at the next launch. A request through the scheme took 0.09 to 0.11 ms, against 0.42 to 0.54 ms to `quillo run` over loopback.

Fix Q-465 first: it is the ask, and the others follow from it. Q-466 costs people their data today on the desktop, so it can be fixed on its own, before any of this is built.

## The answer

Yes, on all six, with one design. The program runs in the app's process with no listening socket. The web view gets its pages from the app itself, through a scheme handler that each system has. What a scheme handler cannot carry, it hands to a message bridge between the page and the app, which every system also has. The program is still written as it is for a server: routes, sessions, `server fn`, live data, channels, uploads. The two things that change are underneath it. The event loop takes requests from the app as well as from sockets. The page's runtime sends through one function, which picks the scheme or the bridge.

But the scheme handlers are not HTTP, and each lacks something different. Measured on WebKit, and read from the APIs on the others:

| | iOS, macOS (WKWebView) | Android (WebView) | HarmonyOS NEXT (ArkWeb) | Windows (WebView2) | Linux (WebKitGTK) |
|---|---|---|---|---|---|
| The page's own requests | `WKURLSchemeHandler` on `quillo://app/` | `shouldInterceptRequest` on a domain the app answers for | `WebSchemeHandler` (ArkTS) or `ArkWeb_SchemeHandler` (NDK, C) | `WebResourceRequested` on `http(s)://quillo.localhost` | `webkit_web_context_register_uri_scheme` |
| A body of text, JSON or bytes | **arrives** (measured) | **never**: the request has no body | in the API (`getHttpBodyStream`); not run | arrives (`Content`, which wry reads) | arrives with wry's `linux-body`, WebKitGTK 2.40+ |
| A body that is a `Blob`, a `FormData` or a form with a file | **lost: 0 bytes** (measured); arrives whole once the page turns it into bytes | never | to check | to check | to check |
| `Set-Cookie`, `document.cookie` | **ignored, both** (measured) | to check | to check | to check | to check |
| WebSocket | **no** (measured) | no: nothing to upgrade | no | no | no |
| An answer sent in parts | in the API (`didReceiveData` again and again); wry sends it whole | in the API (an `InputStream`) | in the API (`didReceiveResponseBody`) | in the API (a stream) | in the API (a `GInputStream`) |
| Service workers | **refused**: "protocol is either HTTP or HTTPS" (measured) | as for https | to check | as for https | refused, as on macOS |
| `<video>` | asks with `Range: bytes=0-1` (measured), so the handler must answer ranges | the same | the same | the same | the same |
| The bridge | `WKScriptMessageHandlerWithReply` (in use) | `addWebMessageListener` (in use) | `javaScriptProxy` or a message port | `postMessage` / wry's `ipc` | wry's `ipc` |

So the design has three rules, one for each gap:

1. **Bodies.** Text and bytes go through the scheme where the system carries them. On WebKit, a `Blob` or a `FormData` is first turned into bytes in the page: `new Response(formData).arrayBuffer()` gives the multipart body with its boundary. All 5,243,163 bytes arrived that way (run 4). On Android every body goes through the bridge.
2. **Cookies.** The web view keeps none. The runtime keeps the jar, since an app has one person: it takes `Set-Cookie` off each answer and puts `Cookie` on each request, as `site` calls already keep theirs in an app of one person (`.quillo/sites/<name>.json`, Q-412).
3. **Sockets.** Live data and channels go through the bridge: a connection is a numbered stream of messages each way.

### What the socket costs today

| | Today | Issue |
|---|---|---|
| A program inside an app with no socket | not possible: both launchers call `server::serve_with` on 127.0.0.1, and the answer path is crate-private and tied to a `TcpStream` | Q-465 |
| The page's origin | `http://127.0.0.1:<port>`: on the desktop a new port at each launch, so `localStorage` and IndexedDB start empty each time; on a phone the last port, only while it is free | Q-466 |
| An iPhone app back from a long time in the background | iOS may take back a suspended app's listening socket; nothing in Quillo notices, or listens again | Q-467 |
| Jobs and ticks | the timer thread reaches the program with a TCP request to itself | Q-468 |
| Who else can reach the program | any app on the phone, and any page in the phone's browser: the launch gate answers 403, but the port is open and the program's loop reads what they send | Q-469 |
| Channels where there is no WebSocket | nothing: live data falls back to a long poll; channels have no fallback | Q-470 |
| Plain HTTP to 127.0.0.1 | an exception on iOS (local networking) and Android (cleartext config); on HarmonyOS, the plan's open question | – |

### Measured: one WKWebView, a page from `q://localhost`, and the same page from `quillo run`

Four launches of the probe, on macOS 26.7.1. Runs 1 and 2 are the same page; run 2 checks what run 1 left behind.

| What the page did | What arrived, or what the page saw |
|---|---|
| `location.origin`, `isSecureContext`, `crypto.subtle` | `q://localhost`, `true`, there |
| `localStorage.getItem("k")` at launch 2, set at launch 1 | `"run1"`: kept |
| IndexedDB open | ok |
| 500 `fetch("/ping")` in a row, then 2,000 | 0.106 and 0.090 ms each; 0.091 and 0.100 ms |
| The same against `quillo run` on 127.0.0.1:4391 (`route GET /ping = "pong"`) | 0.544 ms (500); 0.448 and 0.420 ms (2,000) |
| `fetch` POST, JSON of 1,008 bytes | 1,008 bytes, `application/json` (WebKit gave `HTTPBody`) |
| `fetch` POST, `URLSearchParams` of 1,002 bytes | 1,002 bytes |
| `fetch` POST, a `Uint8Array` of 5 MB | 5,242,880 bytes |
| `fetch` POST, a `Blob` of 5 MB | **0 bytes**, no content type. WebKit gave neither `HTTPBody` nor `HTTPBodyStream` (patched wry, runs 2 to 4) |
| `fetch` POST, a `FormData` with a 5 MB file | **0 bytes**, though the content type names a boundary. Neither body |
| A `<form method=post enctype=multipart/form-data>` with 100,000 characters of text, submitted | 100,276 bytes |
| The same form with a 5 MB file in it, submitted | **0 bytes**. Neither body |
| The 5 MB `FormData` and the form, each turned into bytes with `new Response(fd).arrayBuffer()`, the content type sent with them | 5,243,163 bytes each, with the boundary |
| `Set-Cookie: a=1; Path=/; HttpOnly` on a `fetch` answer, then a `fetch` | no `Cookie` header |
| `Set-Cookie: nav=1` on a page's answer, then a link to another page | no `Cookie` header |
| `document.cookie = "b=2; path=/"`, then reading `document.cookie` | `""` |
| `new WebSocket("ws://localhost/ws")` from the scheme's page | `error` |
| `new EventSource("/sse")` (the answer is whole: `data: hi`) | `message hi` |
| `fetch("/chunks")`, 1 MB, read with a reader | 1 read of 1,048,576 bytes (wry sends it whole) |
| `navigator.serviceWorker.register("/sw.js")` | refused: the script's URL must be HTTP or HTTPS |
| `<video src="/v.mp4">` | two requests, `Range: bytes=0-1` then `bytes=0-1445` |
| `history.pushState(null, "", "/somewhere?x=1")` | `q://localhost/somewhere?x=1` |
| `fetch("http://127.0.0.1:4391/ping")` from the scheme's page | `Load failed`: the scheme's page cannot reach a local server either |

The timing compares the transports, not like with like. The scheme's handler was a Rust closure that returned four bytes. The loopback side went through Quillo's routing and its request log too. So 0.33 to 0.45 ms is the most the socket can cost per request, not what it costs. The program's own work stays either way.

## The design: `embed`

### In the runtime

A new module, `src/embed.rs`. It replaces what [src/mobile.rs](../../src/mobile.rs) and [src/desktop.rs](../../src/desktop.rs) do with `serve_with`:

```rust
pub struct Program { /* the loaded App, its event loop on a thread of its own */ }

impl Program {
    /// Copies the bundle (mobile::sync), loads `main`, starts the loop. No socket.
    pub fn open(bundle: &Path, data: &Path, main: &str, device: &str) -> Result<Program, String>;
    /// One request. The answer comes on `reply`, in parts if it streams, on the loop's thread.
    pub fn request(&self, req: HttpRequest, reply: Box<dyn Reply + Send>);
    /// A page opens a connection (live data, channels) through the bridge: the program's
    /// messages to it come on `to_page`, and the page's messages go to `send`.
    pub fn connect(&self, req: HttpRequest, to_page: Box<dyn FnMut(Frame) -> bool + Send>) -> Conn;
    pub fn send(&self, conn: Conn, frame: Frame);
    pub fn close(&self, conn: Conn);
    /// The app goes behind the home screen, and comes back: ticks pause, jobs catch up.
    pub fn suspend(&self);
    pub fn resume(&self);
    pub fn stop(self);
}

pub trait Reply {
    fn head(&mut self, status: u16, headers: &[(String, String)]);
    fn body(&mut self, part: &[u8]) -> bool; // false: the page went away
    fn end(&mut self);
}
```

Inside, the changes are underneath `Worker::answer`, which stays as it is:

- **The event loop takes requests from the app** ([src/httpd.rs](../../src/httpd.rs)). The loop already has a `Waker` token. It gains a queue: `request` pushes onto it and wakes the loop, and the loop runs the request as a coroutine, as it runs one read from a socket. The answer goes to the `Reply` as `ConnSink` writes to a connection today, so `stream` and `events` stream as they do now.
- **A detached answer gets a pipe, not a socket.** `Detached` is `FnOnce(TcpStream, Vec<u8>)` ([src/httpd.rs:367](../../src/httpd.rs)). It becomes `FnOnce(Box<dyn Duplex>, Vec<u8>)`, where `Duplex` is a `TcpStream` or a bridge connection. `serve_live`, `serve_channels`, `serve_events` and `send_file` then work over either. `ws::Socket` frames over a `TcpStream`. Over the bridge, a frame is already a message, so the socket's framing is skipped.
- **Jobs and ticks are put on the queue** (Q-468), not sent over TCP. Servers gain from this as well: no connection per job, and no secret header to check.
- **No launch gate on the embedded path.** Nothing outside the app can reach it, so `launch_token` is `None`. The Origin checks stay (`cross_site`), since other sites' frames load in the same web view (Q-409).
- **The cookie jar.** `Program` keeps the cookies the program sets, in memory and in `<data>/.quillo/cookies.json`, so a session survives a relaunch. It takes `Set-Cookie` off each answer and adds `Cookie` to each request. The jar goes by path and expiry. There is one person, so no domain matching is needed.
- **`request.remote`** is `"app"`. `platform().runtime` stays `"mobile"` or `"app"`.
- **Ranges.** `send_file` already answers `Range`. The scheme handler must pass the header through, and the status (206).

The C ABI in `mobile/quillo-mobile` follows `Program`. `quillo_open` returns a handle. `quillo_request` takes the method, URL, headers and body, and calls back with the head, then the parts, then the end. `quillo_connect`, `quillo_send` and `quillo_close` handle bridge connections, and `quillo_suspend`, `quillo_resume` and `quillo_stop` the rest. The JNI and Node-API (HarmonyOS) entries wrap the same calls. `quillo app` calls `Program` directly: it is the same process.

### In the page: one way out

[runtime.js](../../src/client/runtime.js) gains one object, `Wire`, which every request and connection goes through: `rpc`, `liveLoop`, `chanSocket`, prefetches, `markdown`, `receive`, and navigation's fetches. In a browser it is `fetch` and `WebSocket`, unchanged. In an app the shell says which one to use, in the script it adds before the page's:

| What the page sends | iOS, macOS, Linux, Windows | Android |
|---|---|---|
| A GET (pages, scripts, images, `<video>`, live data's long poll) | the scheme | the scheme |
| A body of text or bytes | the scheme | the bridge |
| A body that is a `Blob`, a `File` or a `FormData` (a `server fn` with files: [runtime.js:4071](../../src/client/runtime.js)) | turned into bytes (`new Response(body)`), then the scheme | turned into bytes, then the bridge |
| Live data, channels | the bridge (a connection) | the bridge |

The bridge carries bytes as base64 where it carries only text. On Android `WebMessageCompat` takes an `ArrayBuffer` where the WebView has `WEB_MESSAGE_ARRAY_BUFFER`.

**A page with no runtime.** A page with no components loads no script (`island_loader`), and its forms post the browser's way. On WebKit that loses a form with a file (measured). On Android it loses every form's body. So the shell's own script, which runs before every page, takes each `submit` that the scheme cannot carry (on Android every POST, on WebKit every form with a file). It sends the body through the bridge, and the runtime answers that request as it would any other and keeps the answer. The script then sends the page to `/_quillo/posted/<id>`, a one-time address the scheme answers with the kept answer (a redirect for the usual post, redirect, get). The back button and a reload then behave as they do after an ordinary post and redirect.

### The origin, once

Each shell gets one origin for good: `quillo://app` on iOS, macOS and Linux, `https://quillo.localhost` on Windows, and on Android an `https` domain the interceptor answers for. Storage then follows the app, not a port (Q-466). Existing installs had `http://127.0.0.1:<port>`: their database is the program's and is not touched, but the pages' `localStorage` and IndexedDB stay with the old origin. The first launch of the new build opens the old origin once, hidden, copies `localStorage` across through the bridge, and records that it did. IndexedDB is not copied; the guide should say so.

### What the program sees: nothing new

Routes, `request`, `session()`, `reply`, `redirect`, uploads (`file` parameters), `server fn`, live data, channels, `stream`, `site` calls out, `http.*` out: all the same. What differs is said in the guide's phone-apps chapter:

- Service workers do not run on a custom scheme. Quillo does not use them; a program's own one will not start.
- A cookie the page's own script reads (`document.cookie`) is empty in an app. The program's cookies still reach the program.
- `request.remote` is `"app"`.
- Debugging with Safari's or Chrome's inspector works as before. The address is `quillo://app/...`, not `http://127.0.0.1:...`.

### And without a web view?

| Way | What it is | For now |
|---|---|---|
| A. The web view, no socket (above) | everything Quillo does keeps working; the system's own web view draws it | **build this** |
| B. A native screen calling the program | the app's own Swift, Kotlin or ArkTS screens call `quillo_request("POST", "/_quillo/rpc/<fn>", json)` and `quillo_connect` to live topics: Quillo is the app's data and logic, the screen is the platform's | comes free with A's ABI; document it, one example |
| C. Quillo components drawn without a web view | an HTML and CSS engine of Quillo's own (as Blitz or Servo are), or components mapped to each system's widgets (as React Native does); components would run in the Rust interpreter with events, a DOM and layout of their own, and `js()`, `getUserMedia`, `MediaRecorder` and `<input type=file>` would be gone | not now: a second runtime as large as the first (client.rs and runtime.js are 10,400 lines), for one platform at a time |
| D. No screen at all | Linux services, kiosks' back ends, tools: `quillo run` and scripts already; with A's ABI, a program in C, Go or Python links Quillo and calls it without a port | comes free with A |

### Per system

- **iOS.** A `WKURLSchemeHandler` of the app's own in Swift (not wry), so an answer can stream (`didReceiveData` per part, as `Reply::body` gives it). It should also stop a task the web view gave up on (`webView(_:stop:)` drops the `Reply`). The bridge already exists ([App.swift](../../mobile/ios/Quillo/App.swift), `quilloNative`); connections are added to it. The ATS exception for local networking goes. The `port` file goes. Q-467 goes with the socket.
- **Android.** `shouldInterceptRequest` for GETs, answered with a `WebResourceResponse` whose `InputStream` the `Reply` writes to (a pipe), so it streams. Everything with a body goes through the bridge, `Bridge.kt`'s listener, which already keeps to the program's origin. The cleartext config goes. To check on the emulator: whether `Set-Cookie` on an intercepted answer reaches `CookieManager` (the jar makes it not matter, but a leftover cookie would be sent twice).
- **HarmonyOS NEXT.** ArkWeb's NDK scheme handler takes the request, body stream included, in C. So Rust can answer it without going through ArkTS. [harmony/PLAN.md](../../mobile/harmony/PLAN.md)'s first open question ("whether ArkWeb serves plain `http://127.0.0.1`") is no longer asked. The scheme is registered with `customizeSchemes` before the web view is made, marked secure and fetch-enabled. From the documentation, not run: there is no HarmonyOS SDK here.
- **macOS, Windows, Linux (`quillo app`).** wry's `with_asynchronous_custom_protocol` and `with_ipc_handler`. wry sends an answer whole, so streaming over the scheme is not there. Long polls work, and live data and channels go through the bridge, so what does not stream is a `stream` route. That is acceptable for a first version, or the platform's handler can be used directly on macOS as on iOS. On Linux, wry's `linux-body` feature and WebKitGTK 2.40 or later; with an older WebKitGTK, bodies go through the bridge, as on Android. On Windows, `with_https_scheme(true)`. The blank white page `quillo app` showed in a Windows 11 VM on 2026-10-30 (x64 under emulation, cause unknown) is worth trying again on this path, in case loopback is the cause.
- **Linux without a desktop** (a kiosk, a board with a screen). WPE WebKit (Cog) registers URI schemes as WebKitGTK does, so the Linux shell's handler carries over. To check.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-339 | The fix keeps the port "while it is free": the origin, and the page's storage with it, still change when it is not (Q-466) | 0 |
| Q-409 | Frames of other sites load in the app's web view, so the embedded path keeps the Origin check though nothing outside the app can reach it | 0 |
| Q-414 | Plain HTTP to 127.0.0.1 is why the apps carry an ATS exception and a cleartext config; with no socket both go | 0 |

## New issues

### Q-465. A program cannot run inside an app without an HTTP server: both launchers listen on 127.0.0.1, and nothing else can hand the program a request

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading src/mobile.rs, src/desktop.rs, src/httpd.rs | 0 |

`mobile::start` ([src/mobile.rs:134](../../src/mobile.rs)) and `desktop::start_program` ([src/desktop.rs:98](../../src/desktop.rs)) both end in `server::serve_with` on `127.0.0.1`, and the web view is pointed at the port with a launch token. `Worker::answer` ([src/server.rs:2302](../../src/server.rs)) takes an `HttpRequest` and could answer one from anywhere. But it is `pub(crate)`, it runs only on the event loop's coroutines, and three kinds of answer need a `TcpStream`: `Answer::Detach` for live data, channels and dev reloads (`waiter`, [src/server.rs:1358](../../src/server.rs)), and large files (`send_file`). So the socket is the only way in, with all it brings: Q-466 to Q-470, the ATS and cleartext exceptions, and the token.

**Suggestion:** `src/embed.rs` as designed above: a queue into the event loop, `Detached` over a `Duplex` trait, a cookie jar, and a C ABI in `quillo-mobile`. Then `mobile::start` and `quillo app` move onto it, with `QUILLO_TRANSPORT=socket` keeping the old path for one release. A test should run every kind of answer both ways and compare them: a page, a redirect, a 304, a `stream`, live data's long poll, a channel, an upload, a range, a `server fn` with files.

### Q-466. `quillo app` takes a new port at each launch, so the window's origin changes and the page's `localStorage` and IndexedDB start empty every time; on a phone they do when the last port is taken

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading src/desktop.rs; the probe's run 2 for why the origin is what matters | 0 |

```rust
// src/desktop.rs:101
let addr = format!("127.0.0.1:{}", o.port.as_deref().unwrap_or("0"));
```

Without `--port`, the system picks a port each time, and a page's storage goes by origin, port included. So a program that keeps a draft, a setting or a cache in `localStorage` finds it gone at the next launch. The phone apps fixed this for themselves (Q-339): `kept_port` ([src/mobile.rs:193](../../src/mobile.rs)) takes the last port if it is free. If it is not (another app took it, or the last launch has not let go), it silently takes another, and the storage is gone for that launch. Neither path says anything. Not run here: the binary at hand is built without `app`.

**Suggestion:** now, keep the port in `quillo app` as the phones do, in the program's `.quillo/` folder, and log a line when it cannot be had ("this launch's pages have a new origin: what they kept in the browser is not there"). For good, the fixed origin of the embedded path.

### Q-467. iOS may take back a suspended app's listening socket, and nothing in Quillo notices or listens again: the app comes back to a page that cannot reach its program

| Kind | Detected by | Attempts |
|---|---|---|
| silent | reading Apple's TN2277 against src/mobile.rs and src/httpd.rs | 0 |

Apple's TN2277 (Networking and Multitasking) says that while an app is suspended, the system may reclaim a listening socket's resources. The socket then accepts nothing, and the app has to close it and listen again when it comes back. [mobile/README.md](../../mobile/README.md) says iOS suspends the program's server with the app. `httpd::serve` listens once. The iOS launcher does not tell the runtime when the app comes back, and nothing checks the listener then. The page's next `fetch`, live poll or `server fn` would fail, as "could not be reached". Not reproduced: the simulator does not reclaim sockets, and the iPhone runs in this report were short.

**Suggestion:** gone with Q-465. Until then, the iOS launcher should call a `quillo_resume` on `UIApplication.willEnterForegroundNotification`. That call checks the listener (a connect to itself) and listens again on the same port if it is dead.

### Q-468. Jobs and ticks reach the program with a TCP request to itself

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading src/jobs.rs | 0 |

`jobs::ask` ([src/jobs.rs:390](../../src/jobs.rs)) opens a connection to the server's own address and sends `POST /_quillo/job/<name>` (or `/tick/<name>`) with a secret header, which `Worker::answer` lets past the launch gate (`is_job_request`). This is how a job runs as a request does, on the loop, waiting without holding anyone up. But it means `schedule` and `every` need a socket, so a program with no socket runs no jobs. On a server each tick costs a connection: `every(16, step)` is about 60 connections a second to itself.

**Suggestion:** the loop's request queue (Q-465), with the job's name in the queued item instead of a URL and a secret. The `/_quillo/job/` route and `is_job_request` then go.

### Q-469. Any app on the phone, and any page in its browser, can reach the program's port

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, privacy | reading src/server.rs (`launch_gate`) and src/mobile.rs | 0 |

The program listens on `127.0.0.1` at a port kept from launch to launch (Q-339). Every app on the phone shares that address, and so does every web page open in the phone's browser. The launch gate ([src/server.rs:2765](../../src/server.rs)) answers anything without the token with 403, so nothing gets in. But the port being open says the app is installed and running, and the port is the same each time. In 2025, researchers showed apps that listened on localhost being reached and used by web pages in the phone's browser to link visits (the "Local Mess" disclosure, about Meta's and Yandex's apps). The program's loop also reads and parses whatever they send, up to the size limits.

**Suggestion:** gone with Q-465. Until then, mobile/README.md should say it, in "What a phone build leaves out".

### Q-470. Channels have no way to the page but WebTransport and WebSocket; live data has a fallback, channels have none

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | reading runtime.js (`chanSocket`, `liveLoop`); the probe's WebSocket | 0 |

`liveLoop` tries a WebSocket and, failing that, long-polls with `fetch` ([runtime.js:4194](../../src/client/runtime.js)), which works over a custom scheme. `chanSocket` ([runtime.js:3625](../../src/client/runtime.js)) opens a WebSocket and has nothing after it. With no socket (the probe: `new WebSocket(...)` gives `error`), and in any network that blocks WebSockets, `channel` and `listen` get nothing. The page does not say why.

**Suggestion:** channels over `Wire`'s connection (the bridge in an app). In a browser, keep WebTransport and WebSocket, and say on the page, once, when neither opens (as live data does: "live updates are not reaching this page").

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| The probe ended a phase when a message held `"error"` | to stop on a failed run | the WebSocket's result was the string `"error"`, so run 1 ended before the form post; matched on the phase's name instead | 1 run |
| Blamed wry for the empty `Blob` and `FormData` bodies | wry reads a body stream only `while hasBytesAvailable()`, which can stop early | the patched wry showed WebKit hands over neither `HTTPBody` nor `HTTPBodyStream` for them: the loss is WebKit's, and wry's loop is a second, smaller bug | 1 build, 1 run |
| `@mount { ... }` as a block in a component's code | other frameworks' habit | parse error; it is a markup attribute, `@mount={() => ...}` | 1 `quillo check` |
| `quillo app` twice, to see the origin change (Q-466) | the plainest repro | the release binary is built without `app`; rebuilding it in the shared `target/` would disturb other sessions, so Q-466 is from reading | 1 run |

## Limitations

- Only WebKit on macOS was run. iOS's WKWebView is the same engine, but the simulator and a phone were not tried. Android's WebView, WebView2, WebKitGTK and ArkWeb are from their APIs and from wry's handlers, so the table marks them "to check".
- wry was the scheme handler, so "an answer in parts" was not measured: wry sends whole bodies. A handler of the app's own is needed to see whether WKWebView hands a `fetch` reader each part as it comes.
- The timing compares a four-byte closure with a Quillo route over loopback (see the note under the table). The embedded path's own cost, the loop's queue and the coroutine, will be known when it is built.
- HarmonyOS: no SDK and no device here. The ArkWeb APIs named are from Huawei's documentation.
- Q-467 is from Apple's documentation. It was not reproduced.

## Edge cases checked that behave well

- A custom scheme's page is a secure context on WebKit: `crypto.subtle` and `getUserMedia` are there.
- Its origin is the same at each launch, and `localStorage` was kept from one launch to the next.
- A multipart form of text posted by the browser itself arrived whole (100,276 bytes), and so did `URLSearchParams` and a JSON string.
- An `EventSource` on the scheme got its message.
- `history.pushState` on the scheme gives a `q://` address, so Quillo's navigation, which pushes the program's paths, carries over.
- `<video>` asks the scheme for ranges, as it asks a server, so `send_file`'s ranges carry over.
- A page on the scheme cannot fetch `http://127.0.0.1`, so a page cannot reach around the app to another local server.

## Probe

One Rust file and a `Cargo.toml` (`wry = "0.57"`, `tao = "0.37"`, `http = "1"`), built with `cargo build --release --offline`. Run as `scheme-probe <run> [loopback-origin]`. Run 1 and 2: the page below, then 500 pings against the loopback origin. Run 3: 2,000 pings each way, then a form with a 5 MB file. Run 4: the bytes workaround. Each request the handler saw is printed with its body's length, content type, `Cookie` and `Range`.

The handler (`q://localhost/...`):

| Path | Answer |
|---|---|
| `/` | the page |
| `/ping` | `pong` |
| `/echo` | `{"len": <body bytes>, "ctype": ..., "cookie": ...}` |
| `/setcookie` | `Set-Cookie: a=1; Path=/; HttpOnly; SameSite=Strict` |
| `/sse` | `text/event-stream`, `data: hi` |
| `/chunks` | 1 MB of `c` |
| `/formpost`, `/formfile` | a page that posts the body length it got back to the app (`window.ipc.postMessage`); `/formfile` sets `nav=1` and goes on to `/after`, which reports the `Cookie` it got |

The page (runs 1 and 2):

```js
R.origin = location.origin; R.secure = isSecureContext; R.subtle = typeof crypto.subtle;
R.ls_before = localStorage.getItem("k"); localStorage.setItem("k", "run" + RUN);
let t = performance.now();
for (let i = 0; i < 500; i++) await (await fetch("/ping")).text();
R.ping_ms = (performance.now() - t) / 500;
const big = new Uint8Array(5 << 20).fill(7);
r = await fetch("/echo", { method: "POST", body: new Blob([big]) });            // 0 bytes
r = await fetch("/echo", { method: "POST", body: big });                        // 5,242,880
const fd = new FormData(); fd.append("f", new Blob([big]), "x.bin"); fd.append("t", "hello");
r = await fetch("/echo", { method: "POST", body: fd });                         // 0 bytes
await fetch("/setcookie"); r = await fetch("/echo");                            // no Cookie
document.cookie = "b=2; path=/"; R.document_cookie_after_write = document.cookie; // ""
new WebSocket(location.origin.replace(/^q:/, "ws:") + "/ws");                   // error
navigator.serviceWorker.register("/sw.js");                                     // refused
const v = document.createElement("video"); v.src = "/v.mp4"; document.body.appendChild(v); // Range: bytes=0-1
```

Run 4, the workaround:

```js
const ser = new Response(fd);
const ct = ser.headers.get("content-type");        // multipart/form-data; boundary=...
const buf = await ser.arrayBuffer();
await fetch("/echo", { method: "POST", headers: { "content-type": ct }, body: buf }); // 5,243,163 bytes
```

The wry patch that showed which body WebKit gave (`src/wkwebview/class/url_scheme_handler.rs`):

```rust
eprintln!("PROBE {} HTTPBody={} HTTPBodyStream={}", method, body.is_some(), body_stream.is_some());
// and the stream read to its end, not while hasBytesAvailable():
loop {
    let count = unsafe { body_stream.read_maxLength(NonNull::new(buf.as_mut_ptr()).unwrap(), buf.len()) };
    if count <= 0 { break; }
    sent_form_body.extend_from_slice(&buf[..count as usize]);
}
```

Every `Blob`, `FormData` and file-form POST printed `HTTPBody=false HTTPBodyStream=false`. Every POST that arrived printed `HTTPBody=true`.
