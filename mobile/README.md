# Quillo on phones

A Quillo program as a phone app: the runtime runs the program inside the app's process,
with no HTTP server, and the app's own web view gets its pages from it through a scheme of
the app's (`quillo://app/` on iOS, `https://quillo.localhost/` on Android). The program
works offline, keeps its SQLite database in the app's container, and is written exactly as
it is for a server (FEEDBACK39.md, Q-465 to Q-470).

| | |
|---|---|
| `quillo-mobile/` | the runtime as a library: `quillo_open`, `quillo_request`, `quillo_bridge` and the rest for iOS (C), `Quillo.open`, `Quillo.request`, ... for Android (JNI); see [The calls into the runtime](#the-calls-into-the-runtime) |
| `ios/` | the iOS app: one screen, a WKWebView on the program; `build.sh` makes Quillo.app for the simulator |
| `android/` | the Android app: the same, with a WebView and the same `window.quilloNative`; `build.sh` makes a debug APK and runs it on an emulator or a phone |
| `harmony/PLAN.md` | how HarmonyOS NEXT would be done; not built |
| `examples/about/` | a page showing what `platform()` says |
| `examples/fieldbook/` | field notes with the place, a photo, a voice memo and the weather, and a probe of 35 things the phone can do (FEEDBACK18.md) |
| `../src/embed.rs` | the program with no socket (`Program`): requests handed in, the bridge's messages, the cookie jar, and the shell's script (`SHELL_JS`) |
| `../src/mobile.rs` | copies the bundled program into the app's container and keeps a session key there; `start`, the old way, serves it on 127.0.0.1 |

## The phone's own hardware

The iOS and Android apps put `window.quilloNative` on the program's pages (their own
origin and app.toml's `site` only, in the main frame), for what a web view cannot reach. A component calls it with `js()` in an inline handler or
`@mount`, which wait for its answer; a refusal is an error value.

| Call | Answer |
|---|---|
| `js("quilloNative.location")` | `{ lat, lon, accuracy, altitude, speed, course, at }` from the phone's location services, asking the person the first time |
| `js("quilloNative.haptic", "success")` | a tap felt: `light`, `medium`, `heavy`, `rigid`, `soft`, `success`, `warning`, `error`, `selection` |
| `js("quilloNative.battery")` | `{ level, state, low_power }`; `level` is null when the system does not say |
| `js("quilloNative.notify", title, body, seconds)` | a local notification, asking the person the first time |
| `js("quilloNative.keepAwake", true)` | the screen stays on while the app is open |
| `js("quilloNative.brightness")`, `("quilloNative.brightness", 0.8)` | the screen's brightness, read or set, 0 to 1; on Android `null` gives it back to the system, as iOS does when the app leaves the screen |
| `js("quilloNative.share", text, url)` | the system's share sheet: `{ shared, to }`, whether it was sent and where (an activity's name on iOS; both `null` on Android, which does not say) |
| `js("quilloNative.openSettings")` | the app's page in Settings, to turn on what was refused |
| `js("quilloNative.wait", ms)` | a pause of up to 60 s, which a component has no other way to take |
| `js("quilloNative.info")` | the calls this app has |

A link the web view cannot open itself, `tel:`, `sms:`, `mailto:`, a maps link (`geo:` on
Android, `maps:` on iOS) or a store's (`market:`, `itms-apps:`), goes to the app the phone
opens it with; other schemes are dropped. An `<iframe>` of another site loads where it
is, in the page, rather than sending the person to Safari or Chrome; a frame gets no
bridge (FEEDBACK33.md, Q-409). The program's pages have one origin at every launch,
`quillo://app` or `https://quillo.localhost`, so `localStorage` and IndexedDB, which go by
origin, keep what they had (Q-466).

The camera, the microphone and recording work through the web's own `getUserMedia` and
`MediaRecorder`, and photos through `<input type=file capture>`: the app grants them to
its own pages, having asked the person once (Info.plist says why). iOS suspends the app,
and the program with it, when it leaves the screen: a `schedule` job waits until it is
back. The app calls `quillo_suspend` then, which writes the cookie jar at once, as the app
may not come back.

## No socket: how the web view reaches the program

Nothing listens, so no other app on the phone and no page in its browser can reach the
program (Q-469), iOS has no listening socket to take back from a suspended app (Q-467),
and the apps carry no exception for plain HTTP: iOS's `NSAllowsLocalNetworking` and
Android's cleartext domain for 127.0.0.1 are gone (`network_security_config.xml` now
refuses cleartext everywhere). Each request goes this way:

| What the page sends | iOS (WKWebView) | Android (WebView) |
|---|---|---|
| A `GET`: pages, scripts, images, `<video>`'s ranges, live data's long poll | the scheme (`WKURLSchemeHandler` on `quillo`) | the scheme (`shouldInterceptRequest` on `https://quillo.localhost`) |
| A body of text or bytes | the scheme | the bridge: the interceptor is given no body |
| A body that is a file, a `Blob` or a `FormData` | made bytes in the page (`new Response(body)`), then the scheme: WebKit hands the scheme none of those | made bytes, then the bridge |
| A form posted by the browser itself | the scheme; one with a file, the bridge | the bridge |
| Live data and channels | the bridge, as a numbered connection with messages each way | the bridge |

The shell's script (`quillo_shell_js`), run before every page, puts nothing on `window`: the
runtime asks for its way out (`fetch` and its sockets) with a `quillo-wire` event, and the
native side hands the page each message with a `quillo-wire-receive` event (`deliver_js`).
On Android, where every body goes through the bridge, it also takes over the page's own
`fetch`, so a script's own request with a body goes the same way: the one global it changes. A form
the scheme cannot carry is sent through the bridge, its answer is kept, and the page goes
to `/_quillo/posted/<id>`, which shows it once, or to its redirect: back and reload behave as
after an ordinary post and redirect. On Android an intercepted answer cannot be a redirect,
so a redirect is a small page that goes on to its address.

What else differs from a server:

- **Cookies.** The web view keeps none for a custom scheme, so the runtime keeps the jar,
  in `.quillo/cookies.json` in the app's data, and a session key in `.quillo-secret`
  beside it (unless `QUILLO_SECRET` is set): a session lasts from one launch to the next.
  Answers reach the web view without `Set-Cookie`, and `document.cookie` is empty to the
  page's scripts.
- **`request.ip` is `"app"`.** `platform().runtime` is still `"mobile"`.
- **Jobs, ticks and calls made later** reach the program with no connection to itself
  (Q-468).
- **A `stream` route's answer arrives whole**, when it ends: `quillo_request` answers whole.
- **Service workers** do not run on a custom scheme. Quillo uses none.
- **The origin is new.** An app installed before kept its pages' `localStorage` and
  IndexedDB under `http://127.0.0.1:<port>`; they stay there and are not copied. The
  program's own database is the program's, and is not touched.
- Safari's and Chrome's inspectors work as before, at the new addresses.

## The calls into the runtime

The program runs in the app's process (`quillo::embed::Program`), answering on threads of
its own. The C calls are in `ios/Quillo/quillo.h`; Android's are the same, through JNI,
in `Quillo.kt`:

| C | Android | |
|---|---|---|
| `quillo_open(bundle, data, main, device, err, len)` | `Quillo.open(bundle, data, main, device)` | copies the bundled program into `<data>/app`, loads `main` (main.qwe when empty) and starts it; `device` is a JSON object of what the app knows of the device, for `platform()`. 0, or 1 with the reason in `err`; on Android `""`, or the reason |
| `quillo_request(method, url, headers_json, body, len)` | `Quillo.request(method, url, headersJson, body)` | one request of the web view's, answered whole: a `QuilloAnswer` (`status`, `headers` as a JSON object, `body`, `len`), freed with `quillo_free_answer`; on Android bytes: the status (4 bytes, big-endian), the length of the headers' JSON (4 bytes), the JSON, then the body |
| `quillo_bridge(message)` | `Quillo.bridge(message)` | a message the shell's script posted |
| `quillo_bridge_next(timeout_ms)` | `Quillo.bridgeNext(timeoutMs)` | the next message for the page, JSON to hand the page in a `quillo-wire-receive` event, or null when none came in that time; freed with `quillo_free_text` |
| `quillo_shell_js(post, bodies)` | `Quillo.shellJs(post, bodies)` | the script to run before every page: `post` names the page's way to send the app a string (`window.webkit.messageHandlers.quilloWire.postMessage`, `quilloWire.postMessage`), and `bodies` is `"bytes"` where the scheme carries bytes but not a `Blob` (WebKit) or `"bridge"` where it carries no body (Android); freed with `quillo_free_text` |
| `quillo_suspend()`, `quillo_resume()` | `Quillo.suspend()` | the app leaves the screen, and comes back |
| `quillo_start(...)` | `Quillo.start(...)` | the old way, a server on 127.0.0.1 behind a launch token: kept for apps built before |

How the shells use them: iOS registers a `WKURLSchemeHandler` for `quillo` that calls
`quillo_request` off the main thread (and drops an answer the web view gave up on), a
`WKScriptMessageHandler` named `quilloWire` that calls `quillo_bridge`, a `WKUserScript` at
document start with the shell's script, and a thread that loops on `quillo_bridge_next(1000)`
and hands each message to the page in a `quillo-wire-receive` event. Android answers
`shouldInterceptRequest` for its origin with `Quillo.request`, listens with
`addWebMessageListener("quilloWire", ...)` for the program's origin only, adds the script
with `addDocumentStartJavaScript`, and posts what `Quillo.bridgeNext` gives back to the page
that last wrote. `quillo app` uses the same `Program` through wry, in the same process.

## The app's name, id, version and icon: app.toml

Each program says what its app is called in an `app.toml` beside its main.qwe, which both
build scripts read (app-config.sh):

```toml
name = "Fieldbook"                  # under the icon, 30 characters at most
id = "org.quillolang.fieldbook"     # the bundle id (iOS) and application id (Android); never changed once published
version = "1.0.0"                   # what people see in the store
build = 1                           # a whole number, higher for each upload to a store
icon = "icon.png"                   # 1024 x 1024 PNG without transparency; Quillo's when left out
main = "mobile.qwe"                 # the file the app starts from, in the program's folder; main.qwe when left out
site = "https://app.example.com"    # a site of yours the web view keeps, with the bridge
env.SHOP_URL = "https://shop.example.com"   # what env("SHOP_URL") gives on the phone
```

`env.NAME = "value"` lines are what `env("NAME")` gives on the phone: the phone has no
environment, and the app's `.env` is not read (nor bundled). Under `quillo run` the same
code takes `NAME` from `.env`. They are public, as everything in an app is: anyone can
unzip it (FEEDBACK33.md, Q-406).

`site` is an https address with no path (`https://app.example.com`, a port allowed).
Pages of that origin stay in the web view, and `window.quilloNative` answers them as it
answers the program's pages. The bundled program still starts first and can link or
redirect there. An app that only wraps a website is turned down by Apple's App Review
(guideline 4.2), so the app should be more than the site (Q-410).

`main` is for a program with a file for each platform: `web.qwe` and `desktop.qwe` for
`quillo run` and `quillo app`, `mobile.qwe` for the phone, over the same code. The whole
folder is bundled, so `main = "apps/phone.qwe"` may say `use ../lib/api.qwe`, as long as
`apps/` has no main.qwe of its own, which would make it a program apart.

A bad value stops the build with what is wrong. Without an `app.toml`, the app is
"Quillo" (`org.quillolang.app`) with Quillo's icon. The icon becomes an asset catalog
(iOS, compiled by `actool`) and launcher icons at every density plus an adaptive icon
(Android).

## What goes into an app

Both systems copy the program's folder with one filter, `app_copy_program` in
app-config.sh: everything but `.env` and `.env.*`, `.git/` (the program's history, old
secrets included), `.quillo/` and any other dotfile (`.well-known/` is kept), databases
(`*.db`, `*.db-wal`, `*.db-shm`, `*.sqlite`), `uploads/`, and a subfolder with a main.qwe
of its own, which is a program apart. The build prints how many files went in and what
it left out; when that includes a `.env`, it adds "a .env is the server's: put public
values in app.toml as env.NAME" (Q-407). Android keeps folders that start with `_`:
Gradle's `ignoreAssetsPattern` is set in app/build.gradle.kts, so a program works the
same on both systems (Q-408).

## iOS

```sh
mobile/ios/build.sh app --run                 # the simulator
mobile/ios/build.sh app --device              # iPhones and iPads (arm64), build/iphoneos/Quillo.app
mobile/ios/build.sh app --archive             # ... and build/<Name>-<version>-<build>.ipa for App Store Connect
mobile/ios/build.sh app --device --run        # ... installed on a connected iPhone (signed only)
```

A device build is what App Store Connect expects: arm64, iOS 17 or later, built with the
installed SDK, the icon, the privacy manifest (`Quillo/PrivacyInfo.xcprivacy`: no tracking,
no data collected by the app itself, file times and disk space read inside its container)
and the build keys Xcode writes (`DTSDKName`, `DTXcode`, ...). It is signed when
`QUILLO_IOS_IDENTITY` (the certificate, as `security find-identity -v -p codesigning` names
it) and `QUILLO_IOS_PROFILE` (the provisioning profile for the app's id) are set; the
entitlements come from the profile, which must be for the app's id. Without them the
build and the .ipa are unsigned, ready to sign once an Apple Developer account is active.
A signed .ipa goes up with Apple's Transporter app or `xcrun altool --upload-app`.

### On your own iPhone, with a free Apple ID

No paid account is needed to run on your own phone; the app stops launching after 7 days,
until it is installed again.

1. Turn on Developer Mode on the phone (Settings, Privacy & Security), and connect it.
2. Make a small Xcode project with the app's id, sign in under Signing & Capabilities (Team:
   your Personal Team), and run it on the phone once. Xcode makes the certificate and a
   profile for that id and phone, in `~/Library/Developer/Xcode/UserData/Provisioning Profiles`.
3. Build with them:

```sh
QUILLO_IOS_IDENTITY="Apple Development: Your Name (ABCDE12345)" \
QUILLO_IOS_PROFILE="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles/<uuid>.mobileprovision" \
mobile/ios/build.sh app --device --run
```

4. The phone must be unlocked. The first launch needs your developer profile trusted, once:
   Settings, General, VPN & Device Management.

`QUILLO_IOS_DEVICE` names the phone when several are connected (`xcrun devicectl list devices`).

### In the simulator

```sh
mobile/ios/build.sh app --run                 # app/main.qwe, the to-do list
xcrun simctl launch booted org.quillolang.app --eval "$(cat mobile/ios/add-todos.js)"
```

`build.sh` builds the runtime for `aarch64-apple-ios-sim` (`rustup target add
aarch64-apple-ios-sim` once), copies the program's folder into the app (see [What goes
into an app](#what-goes-into-an-app)), links it all with `swiftc` (no Xcode
project) and, with `--run`, installs and opens it on a simulator. `--eval <script>` runs a
script in the page once it has loaded, for screenshots and tests; given several times, the
scripts run in turn, one per page loaded.

What happens at launch: the program is copied into
`Library/Application Support/quillo/app`; files the program writes (its databases) are
never overwritten by an app update, while its code follows the bundle. `quillo_open`
starts it with no socket, and the web view opens `quillo://app/`. When iOS suspends the
app, the program is suspended with it (`quillo_suspend` first), and answers again when the
app comes back.

## Android

```sh
mobile/android/build.sh app --run      # a debug APK around app/, installed with adb and opened
```

`MainActivity` unpacks the program from the APK's assets into the app's files (assets are
not files), gathers the device's facts (`Build`, the display, memory, locale), starts the
runtime through JNI (`Quillo.open`, `Java_org_quillolang_app_Quillo_open` in quillo-mobile)
and shows a WebView on `https://quillo.localhost/`, whose requests it answers with
`Quillo.request`: a `.localhost` name never leads to the network. Links to other sites
open in the browser, but for app.toml's `site`, whose pages stay in the web view, and
frames, which load where they are; the back button goes back in the program's pages first.
The web view may use cleartext HTTP nowhere (res/xml/network_security_config.xml); the
runtime's own sockets are not the web view's. `onStop` calls `Quillo.suspend`.

- **The bridge** (Bridge.kt): `window.quilloNative` with the same calls as on iOS, put on
  the program's pages before their own scripts and answering only the program's origin
  (`WebViewCompat.addWebMessageListener`, `addDocumentStartJavaScript`), and beside it
  `quilloWire`, the program's own way out for bodies, forms and connections. Haptics go
  through the view (no vibration permission); a notification in N seconds is an alarm,
  delivered while the app is away.
- **Permissions**: the camera, the microphone, location and notifications are asked of the
  person the first time a page or the bridge needs them, with the system's own prompt. The
  page's `navigator.geolocation` is granted with the app's location permission: the person
  is asked once, not twice as on iOS.
- **Files**: `<input type="file">` opens the system's picker; with `capture` and images
  accepted, the camera, whose photo reaches the page through a FileProvider.
- **Edge to edge**: Android 15 draws apps under the system bars; the page is kept clear of
  them and of the keyboard.
- **Back**: back goes back in the program's pages, then leaves the app; on Android 13 and
  later through the system's back callback (predictive back, the only way from Android 16).
- **Tests**: `--es eval "<js>"` (then `eval2`, `eval3`, ...) runs scripts in the pages as
  they load, as `--eval` does on iOS; a long one goes as `"base64:$(base64 < x.js)"`.
  Debug builds only: the activity is open to other apps, which must not run scripts in it.
  An emulator can be tapped (`adb shell input tap`), so the system's prompts can be
  answered in a test, which the iOS simulator cannot do.

Releases:

```sh
mobile/android/build.sh app --release         # build/outputs/<Name>-<version>-<build>.apk (signed) and .aab for Google Play
```

A release targets Android 16 (API 36), as Google Play requires, and runs from Android 8
(API 26); its native library is aligned for 16 KB pages, as Play also requires. It is
signed with the upload key in `QUILLO_ANDROID_KEYSTORE` (with `QUILLO_ANDROID_KEY_ALIAS`
and `QUILLO_ANDROID_STORE_PASSWORD`); without them, a key is made once for the app's id in
`~/.quillo/android/<id>-upload.jks`, its password beside it, and used from then on. Keep
that key backed up: Google Play knows the app by it (with Play App Signing, a lost upload
key can be reset through Play's support).

It needs the Android SDK (platform 36, build-tools, the NDK), a JDK 17, `cargo-ndk` and
the Rust targets `aarch64-linux-android` and `x86_64-linux-android`; Gradle comes from
the wrapper. On a Mac: `brew install --cask android-commandlinetools`, then
`sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;27.2.12479018"`
(and `"emulator" "system-images;android-35;google_apis;arm64-v8a"` for an emulator). Tried
on the Android 15 emulator: Fieldbook's probe, notes with the place, a photo from the
camera and a voice memo, the share sheet, the location prompt refused and allowed.

In the background Android does not stop the app at once, as iOS does: the program went on
answering, and a `schedule` job ran each minute while the app sat behind the home screen
(on the emulator, plugged in, measured when the program was served on 127.0.0.1). The system may still stop the process later to save
battery or memory; the program starts again from its database when the app is opened.

| When | Runs of the minute job |
|---|---|
| start | 7 |
| 70 s on the screen | 8 |
| 150 s behind the home screen | 11 |
| 70 s after coming back | 12 |

## Calling a server

A page's `http.*` to another site meets CORS, and the page's origin, `quillo://app` or
`https://quillo.localhost`, is not one a site can safely allow. When it fails, it says so: `http: no answer from <host>: it
may not allow this page (CORS), or be plain http, or be unreachable; a server fn can call
it from the server` (Q-411). So the program reaches a server from a `server fn`, with the
program's own `http.*`, not from the page.

A deployed Quillo site is declared with `site` and its `server fn`s are called as the
program's own (src/sites.rs, Q-412):

```quillo
site shop = env("SHOP_URL", "https://shop.example.com")   // env.SHOP_URL in app.toml

server fn latest(n: int) = shop.notes(n)                  // the site's `server fn notes`

component Notes() {
    state notes = latest(20)
    <ul>{for n in notes => <li key={n.id}>{n.text}</li>}</ul>
}
```

- `shop.notes(a, b)` is `POST <url>/_quillo/rpc/notes` with a JSON array of the
  arguments. It gives the answer's value, the site's error as an error value, or an
  error value when the site cannot be reached (`site shop: no answer from ...`).
  Decimals and big ints cross as they do for pages.
- The address is any expression. `site` stays an ordinary name elsewhere: `let site =
  ...` works.
- In an app of one person (a phone app, `quillo app`, or with `QUILLO_SITE_COOKIES=1`)
  the cookies the site sets are kept in `.quillo/sites/<name>.json` in the program's
  folder, so whoever signed in stays signed in across launches. On a server they are
  not kept, and a log line says so once: one visitor's session would be every
  visitor's.
- It runs on the server side only: `quillo check` refuses `shop.x()` in a component
  (call it in a server fn).
- `/_quillo/rpc/<fn>` is a stable wire for programs: a JSON array in, `{"ok": v}` or
  `{"err": e}` out. A program sends no `Origin` header and needs none; a request with
  another site's `Origin` is refused.
- A `service` on a phone always runs inside the app. To use a server's, call the server
  with `site`, not `service` (Q-413).

Reaching a server (Q-414):

- Info.plist has `NSLocalNetworkUsageDescription`: a real iPhone asks the person before
  the app reaches a computer on the local network. The simulator shares the Mac's
  network, so `127.0.0.1` reaches a server on the Mac there.
- The runtime's `http.*` and `site` calls trust the certificate roots built into the
  binary, not the phone's, so a company's CA installed on the phone is not trusted by
  them. A call can name its own with `ca`, a PEM file in the program's folder (`ca:
  "system"`, the machine's store, has not been tried on a phone).
- App Transport Security does not cover the runtime's own sockets, so the program can
  reach plain `http://`. Prefer https.

## What a phone build leaves out

- Compiled numbers: iOS forbids writable code, so there is no JIT (built without the `jit`
  feature); the interpreter runs everything.
- Channels over WebTransport: pages use the app's bridge.
- A `stream` route's answer sent in parts: it arrives whole.
- Service workers: none runs on the app's scheme.
- `.qwer` plugins built at run time: there is no compiler on a phone. Linking plugins into
  the app at build time is planned.
- Anything that starts a process: `--instances`, `--split`, `--with`, `--watch`.
- MySQL and PostgreSQL clients are there, but a phone app keeps its data in SQLite.

## Not done yet

- iOS: signing and an upload, waiting on an Apple Developer account; tried on a real iPhone
  when the program was served on 127.0.0.1.
- The shells without a socket on a real phone: run on the iOS simulator and the Android
  emulator (pages, a server fn, live data, a form with a file, and `localStorage` and the
  session kept from one launch to the next), not yet on a real device.
- Copying an old install's `localStorage` to the new origin.
- Android: an upload to Google Play; tried on real phones.
- Only the permissions a program uses (both apps ask for camera, microphone and location).
- HarmonyOS NEXT: see harmony/PLAN.md.
- Linking `.qwer` plugins into the app.

## Where the program runs: `platform()`

A program can ask: `platform()` gives the system, its version, the device, the hardware
model, simulator or not, the processor, the screen, the locale and time zone, the app's
version, and whether it runs in a phone app, `quillo app` or as a server. The iOS launcher
hands over what only it knows (`Program.device` in ios/Quillo/App.swift) when it starts the
program; the runtime reads the rest. `examples/about/` is a page that shows it all:

```sh
mobile/ios/build.sh mobile/examples/about --run
```
