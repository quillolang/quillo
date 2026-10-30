# HarmonyOS: the plan

Not built yet. This is how a Quillo program would run inside a HarmonyOS app, the same
way it runs inside the iOS and Android apps: the runtime linked into the app, serving
the program on 127.0.0.1 to the app's own web view, with `window.quilloNative` on the
program's pages for what a web view cannot reach.

## Which HarmonyOS

| | |
|---|---|
| **HarmonyOS 2 to 4** (and their AOSP base) | Run Android apps: the Android app in `../android` installs as it is. Nothing to build. |
| **HarmonyOS NEXT / 5 and later** (API 12+) | No Android layer: apps are ArkTS (`.hap` packages), built with DevEco Studio's tools. This plan is for these. |

## The pieces, against iOS and Android

| | iOS | Android | HarmonyOS NEXT |
|---|---|---|---|
| The app | Swift, `swiftc` | Kotlin, Gradle | ArkTS, ArkUI, built by hvigor (DevEco's command-line tools) |
| The web view | WKWebView | WebView (Chromium) | the ArkWeb `Web` component (Chromium) |
| The runtime | `libquillo_mobile.a`, C entry | `libquillo_mobile.so`, JNI | `libquillo_mobile.so`, a Node-API module (`napi-ohos`), or the C entry called from a small C++ Node-API shim |
| Rust target | `aarch64-apple-ios(-sim)` | `aarch64-linux-android` | `aarch64-unknown-linux-ohos` (and `x86_64-unknown-linux-ohos` for the emulator on Intel), with the OpenHarmony SDK's clang as the linker |
| The program | the app bundle | APK assets, unpacked | `resources/rawfile/program`, unpacked into `context.filesDir` (as on Android) |
| Plain HTTP to 127.0.0.1 | ATS exception for local networking | network security config | to check: ArkWeb's policy for cleartext to localhost |
| Bridge | `WKScriptMessageHandlerWithReply`, the frame's origin checked | `WebViewCompat.addWebMessageListener` for one origin | `javaScriptProxy` / `registerJavaScriptProxy`, its URL permission list set to the program's origin; async methods return Promises (API 12+) |
| Script before the page's | `WKUserScript` at document start | `addDocumentStartJavaScript` | `runJavaScriptOnDocumentStart` (API 11+) |
| Camera, microphone | `requestMediaCapturePermission` | `onPermissionRequest` | `onPermissionRequest`, with `ohos.permission.CAMERA` and `MICROPHONE` asked through `abilityAccessCtrl` |
| Page geolocation | the web view asks again | granted with the app's permission | `onGeolocationShow`, granted with the app's `APPROXIMATELY_LOCATION`/`LOCATION` |
| `<input type=file>`, `capture` | the system's own | the picker; the camera through a FileProvider | `onShowFileSelector`: the picker (`photoAccessHelper`), the camera (`cameraPicker`) for `capture` |
| Launch scripts for tests | `--eval` arguments | `--es eval` extras | `aa start ... --ps eval <js>` parameters |

## `window.quilloNative`, call by call

| Call | HarmonyOS API |
|---|---|
| `location` | `@ohos.geoLocationManager.getCurrentLocation` |
| `haptic` | `@ohos.vibrator` (preset effects: `haptic.clock.timer` and the like), with `ohos.permission.VIBRATE` |
| `battery` | `@ohos.batteryInfo` (`batterySOC`, `chargingStatus`) and `@ohos.power` (power-saving mode) |
| `notify` | `@ohos.notificationManager` now; `@ohos.reminderAgentManager` for one in N seconds, delivered while the app is away |
| `keepAwake` | `window.setWindowKeepScreenOn` |
| `brightness` | `window.setWindowBrightness`, read from the window's properties |
| `share` | `@kit.ShareKit` (`systemShare`) |
| `openSettings` | a `Want` to the app's page in Settings |
| `wait`, `info` | the page's own script, as on iOS and Android |

## The runtime

- `platform()`: Rust says `target_os = "linux"`, `target_env = "ohos"` there, so
  `src/platform.rs` needs an `"harmonyos"` case before Linux's (`os_release()` would find
  nothing). The app's launcher JSON gives the rest from `@ohos.deviceInfo`
  (`marketName`, `osFullName`, `sdkApiVersion`, `deviceType`).
- TLS roots: `rustls-native-certs` reads Linux paths; HarmonyOS keeps its roots in
  `/etc/ssl/certs/cacert.pem` (to check on a device). Bundling `webpki-roots` for this
  target is the fallback.
- Native dependencies: ring, bundled SQLite and the image codecs are C built by the
  `cc` crate, which takes the SDK's clang through `CC_aarch64_unknown_linux_ohos`; no
  JIT (as on iOS), no WebTransport.
- `mobile/quillo-mobile`: a `napi-ohos` module with `start(bundle, data, main, device)`
  beside the C and JNI entries, behind `cfg(target_env = "ohos")`.

## Steps

1. Get DevEco Studio's command-line tools and the HarmonyOS SDK (a Huawei developer
   account is needed to download them) and an emulator image; sign debug builds with
   the automatic debug certificate.
2. Build `quillo-mobile` for `aarch64-unknown-linux-ohos` and the emulator's target;
   fix what the target needs in `platform.rs` and the TLS roots.
3. `mobile/harmony/`: an ArkTS app (one `Web` component, the program unpacked from
   rawfile, the Node-API call to start it), `build.sh <program> [--run]` with hvigor and
   `hdc` (install, start, `hdc shell snapshot_display` for screenshots).
4. The bridge, the permissions, the file selector and the camera, call by call.
5. Fieldbook on it: the probe, a note with place, photo and memo, the share sheet, the
   app in the background; then a report as for iOS (FEEDBACK18.md) and Android.

## What to find out first

- Whether ArkWeb serves plain `http://127.0.0.1` to the app without a special setting.
- Whether `javaScriptProxy`'s permission list restricts by origin for every frame, as
  the Android listener does.
- How long the system keeps the app's process (and the program's server) running in
  the background before it is frozen.
