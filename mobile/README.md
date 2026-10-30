# Quillo on phones (branch mobile)

A Quillo program as a phone app: the runtime runs inside the app and serves the program
on 127.0.0.1 to the app's own web view, so the program works offline, keeps its SQLite
database in the app's container, and is written exactly as it is for a server.

| | |
|---|---|
| `quillo-mobile/` | the runtime as a library: `quillo_start(bundle, data, main, out, len)` for iOS (C), `Quillo.start` for Android (JNI) |
| `ios/` | the iOS app: one screen, a WKWebView on the program; `build.sh` makes Quillo.app for the simulator |
| `android/` | the Android app: the same, with a WebView (needs the Android SDK and NDK to build) |
| `../src/mobile.rs` | what `start` does: copies the bundled program into the app's container, keeps a session key there, and serves the program to the web view only |

## iOS, in the simulator

```sh
mobile/ios/build.sh app --run                 # app/main.qwe, the to-do list
xcrun simctl launch booted org.quillolang.app --eval "$(cat mobile/ios/add-todos.js)"
```

`build.sh` builds the runtime for `aarch64-apple-ios-sim` (`rustup target add
aarch64-apple-ios-sim` once), copies the program's folder into the app (not its databases,
nor subfolders that are programs of their own), links it all with `swiftc` (no Xcode
project) and, with `--run`, installs and opens it on a simulator. `--eval <script>` runs a
script in the page once it has loaded, for screenshots and tests.

What happens at launch: the program is copied into
`Library/Application Support/quillo/app`; files the program writes (its databases) are
never overwritten by an app update, while its code follows the bundle. The server listens
on 127.0.0.1 at a port the system picks and answers only the web view (a token in the
first URL, then a cookie); other apps get 403. When iOS suspends the app, the server is
suspended with it, and answers again when the app comes back.

## What a phone build leaves out

- Compiled numbers: iOS forbids writable code, so there is no JIT (built without the `jit`
  feature); the interpreter runs everything.
- Channels over WebTransport: pages use the WebSocket.
- `.qwer` plugins built at run time: there is no compiler on a phone. Linking plugins into
  the app at build time is planned.
- Anything that starts a process: `--instances`, `--split`, `--with`, `--watch`.
- MySQL and PostgreSQL clients are there, but a phone app keeps its data in SQLite.

## Not done yet

- A device build (signing, an Xcode project or `xcodebuild` archive) and the App Store.
- Android: `android/` is written but not built here (no SDK on this machine).
- Linking `.qwer` plugins into the app.
