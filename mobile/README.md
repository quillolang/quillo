# Quillo on phones (branch mobile)

A Quillo program as a phone app: the runtime runs inside the app and serves the program
on 127.0.0.1 to the app's own web view, so the program works offline, keeps its SQLite
database in the app's container, and is written exactly as it is for a server.

| | |
|---|---|
| `quillo-mobile/` | the runtime as a library: `quillo_start(bundle, data, main, out, len)` for iOS (C), `Quillo.start` for Android (JNI) |
| `ios/` | the iOS app: one screen, a WKWebView on the program; `build.sh` makes Quillo.app for the simulator |
| `android/` | the Android app: the same, with a WebView; `build.sh` makes a debug APK (needs the Android SDK and NDK) |
| `examples/about/` | a page showing what `platform()` says |
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

## Android

```sh
mobile/android/build.sh app --run      # a debug APK around app/, installed with adb
```

The same design: `MainActivity` unpacks the program from the APK's assets into the app's
files (assets are not files), gathers the device's facts (`Build`, the display, memory,
locale), starts the runtime through JNI (`Quillo.start`, `Java_org_quillolang_app_Quillo_start`
in quillo-mobile) and shows a WebView on the URL it gives; links to other sites open in the
browser, and the back button goes back in the program's pages first. 127.0.0.1 is the only
place cleartext HTTP is allowed (res/xml/network_security_config.xml). It needs the Android
SDK and NDK, Gradle, `cargo-ndk` and the Rust targets `aarch64-linux-android` and
`x86_64-linux-android`. Status: written, the JNI layer type-checked for
`aarch64-linux-android`; the Kotlin and Gradle side has not been built yet.

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
- Android: built and run on an emulator (`android/` is written, not yet built: no SDK on the branch's machine).
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
