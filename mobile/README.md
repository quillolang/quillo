# Quillo on phones

A Quillo program as a phone app: the runtime runs inside the app and serves the program
on 127.0.0.1 to the app's own web view, so the program works offline, keeps its SQLite
database in the app's container, and is written exactly as it is for a server.

| | |
|---|---|
| `quillo-mobile/` | the runtime as a library: `quillo_start(bundle, data, main, out, len)` for iOS (C), `Quillo.start` for Android (JNI) |
| `ios/` | the iOS app: one screen, a WKWebView on the program; `build.sh` makes Quillo.app for the simulator |
| `android/` | the Android app: the same, with a WebView and the same `window.quilloNative`; `build.sh` makes a debug APK and runs it on an emulator or a phone |
| `harmony/PLAN.md` | how HarmonyOS NEXT would be done; not built |
| `examples/about/` | a page showing what `platform()` says |
| `examples/fieldbook/` | field notes with the place, a photo, a voice memo and the weather, and a probe of 35 things the phone can do (FEEDBACK18.md) |
| `../src/mobile.rs` | what `start` does: copies the bundled program into the app's container, keeps a session key there, and serves the program to the web view only |

## The phone's own hardware

The iOS and Android apps put `window.quilloNative` on the program's pages (their own
origin only), for what a web view cannot reach. A component calls it with `js()` in an inline handler or
`@mount`, which wait for its answer; a refusal is an error value.

| Call | Answer |
|---|---|
| `js("quilloNative.location")` | `{ lat, lon, accuracy, altitude, speed, course, at }` from the phone's location services, asking the person the first time |
| `js("quilloNative.haptic", "success")` | a tap felt: `light`, `medium`, `heavy`, `rigid`, `soft`, `success`, `warning`, `error`, `selection` |
| `js("quilloNative.battery")` | `{ level, state, low_power }`; `level` is null when the system does not say |
| `js("quilloNative.notify", title, body, seconds)` | a local notification, asking the person the first time |
| `js("quilloNative.keepAwake", true)` | the screen stays on while the app is open |
| `js("quilloNative.brightness")`, `("quilloNative.brightness", 0.8)` | the screen's brightness, read or set |
| `js("quilloNative.share", text, url)` | the system's share sheet |
| `js("quilloNative.openSettings")` | the app's page in Settings, to turn on what was refused |
| `js("quilloNative.wait", ms)` | a pause of up to 60 s, which a component has no other way to take |
| `js("quilloNative.info")` | the calls this app has |

The camera, the microphone and recording work through the web's own `getUserMedia` and
`MediaRecorder`, and photos through `<input type=file capture>`: the app grants them to
its own pages, having asked the person once (Info.plist says why). iOS suspends the app,
and the program's server with it, when it leaves the screen: a `schedule` job waits until
it is back.

## The app's name, id, version and icon: app.toml

Each program says what its app is called in an `app.toml` beside its main.qwe, which both
build scripts read (app-config.sh):

```toml
name = "Fieldbook"                  # under the icon, 30 characters at most
id = "org.quillolang.fieldbook"     # the bundle id (iOS) and application id (Android); never changed once published
version = "1.0.0"                   # what people see in the store
build = 1                           # a whole number, higher for each upload to a store
icon = "icon.png"                   # 1024 x 1024 PNG without transparency; Quillo's when left out
```

A bad value stops the build with what is wrong. Without an `app.toml`, the app is
"Quillo" (`org.quillolang.app`) with Quillo's icon. The icon becomes an asset catalog
(iOS, compiled by `actool`) and launcher icons at every density plus an adaptive icon
(Android).

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

### In the simulator

```sh
mobile/ios/build.sh app --run                 # app/main.qwe, the to-do list
xcrun simctl launch booted org.quillolang.app --eval "$(cat mobile/ios/add-todos.js)"
```

`build.sh` builds the runtime for `aarch64-apple-ios-sim` (`rustup target add
aarch64-apple-ios-sim` once), copies the program's folder into the app (not its databases,
nor subfolders that are programs of their own), links it all with `swiftc` (no Xcode
project) and, with `--run`, installs and opens it on a simulator. `--eval <script>` runs a
script in the page once it has loaded, for screenshots and tests; given several times, the
scripts run in turn, one per page loaded.

What happens at launch: the program is copied into
`Library/Application Support/quillo/app`; files the program writes (its databases) are
never overwritten by an app update, while its code follows the bundle. The server listens
on 127.0.0.1 at a port the system picks and answers only the web view (a token in the
first URL, then a cookie); other apps get 403. When iOS suspends the app, the server is
suspended with it, and answers again when the app comes back.

## Android

```sh
mobile/android/build.sh app --run      # a debug APK around app/, installed with adb and opened
```

`MainActivity` unpacks the program from the APK's assets into the app's files (assets are
not files), gathers the device's facts (`Build`, the display, memory, locale), starts the
runtime through JNI (`Quillo.start`, `Java_org_quillolang_app_Quillo_start` in quillo-mobile)
and shows a WebView on the URL it gives. Links to other sites open in the browser, and the
back button goes back in the program's pages first. 127.0.0.1 is the only place cleartext
HTTP is allowed (res/xml/network_security_config.xml).

- **The bridge** (Bridge.kt): `window.quilloNative` with the same calls as on iOS, put on
  the program's pages before their own scripts and answering only the program's origin
  (`WebViewCompat.addWebMessageListener`, `addDocumentStartJavaScript`). Haptics go
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

In the background Android does not stop the app at once, as iOS does: the program's server
went on answering, and a `schedule` job ran each minute while the app sat behind the home
screen (on the emulator, plugged in). The system may still stop the process later to save
battery or memory; the program starts again from its database when the app is opened.

| When | Runs of the minute job |
|---|---|
| start | 7 |
| 70 s on the screen | 8 |
| 150 s behind the home screen | 11 |
| 70 s after coming back | 12 |

## What a phone build leaves out

- Compiled numbers: iOS forbids writable code, so there is no JIT (built without the `jit`
  feature); the interpreter runs everything.
- Channels over WebTransport: pages use the WebSocket.
- `.qwer` plugins built at run time: there is no compiler on a phone. Linking plugins into
  the app at build time is planned.
- Anything that starts a process: `--instances`, `--split`, `--with`, `--watch`.
- MySQL and PostgreSQL clients are there, but a phone app keeps its data in SQLite.

## Not done yet

- iOS: signing and an upload, waiting on an Apple Developer account; tried on a real iPhone.
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
