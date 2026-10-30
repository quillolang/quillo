# Feedback from building a phone app in Quillo, and trying the phone's hardware from it

| | |
|---|---|
| App | [mobile/examples/fieldbook/](../../mobile/examples/fieldbook): field notes on a phone. A note has a title, the place (from the app or from the web view), the compass heading, a photo with a thumbnail made on the phone, a voice memo, and the weather at that place from a web API. It also has a probe page that tries 35 things a phone can do and keeps the answers, a page of what `platform()` says, a job every minute, and a CSV export. 638 lines in 11 files. |
| Quillo | 0.6.0 (fc8bfb9, 2026-10-30), branch `mobile` |
| Model | Claude (Fable 5.1) |
| Tested with | `quillo check`; the iOS app from `mobile/ios/build.sh` on the iOS 27 simulator (iPhone, `iPhone19,2`), driven by `--eval` scripts and `simctl` (location, privacy grants, screenshots), its database read with `sqlite3` from the app's container; **a real iPhone 17 Pro Max on iOS 26.7.1**, signed with a free Apple ID and used by hand, its database copied off with `devicectl`; the Android app on the Android 15 emulator; the repros under `feedback/run.mjs` (headless Chrome) |

What worked well: the program is an ordinary Quillo program, and nearly all of it ran on the
phone unchanged. Uploads from a component (a photo and a voice memo, as files given to a
`server fn`) were saved into the app's own container; `image.convert` made the thumbnail on the
phone with no outside tool; `http.get` over TLS fetched the weather; `schedule("1m", ...)` ran
while the app was open; SQLite kept everything across launches and reinstalls; `live()` kept the
list current. The phone's own hardware (location, haptics, battery, brightness, the screen kept
on, the share sheet, notifications) was reached through a small bridge the iOS app puts on the
page, `window.quilloNative`, called with `js()` like any page script, its promises awaited.

Reports 16 and 17 are not in this repository; this one is numbered 18 as asked, and its issues
continue from Q-099, the last in [INDEX.md](../INDEX.md).

## Seen again

None. [Q-054](../INDEX.md) (a name the page lacks is an error value from `js()`) is a fix, not an
issue, but it cost a run: see "Where the AI struggled".

## New issues

### Q-105. `state x = js(...)` is always null on the page

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a screenshot (the battery said "null"), then a browser repro | 2 |

A component's state initializer runs where the component is rendered first: on the server,
where `js()` gives `null` (the reference says so). The browser then resumes the component with
that `null` and does not run the initializer again. So any state read from the page this way
is `null` for good, with no error. On the phone the battery read this way showed "level not
said, null", while the same call in a handler said "unknown".

The js reference's own first example does exactly this and cannot work: `state dark =
js("localStorage.getItem", "theme") == "dark"` is always `false` on a server-rendered page.

```quillo
component Reads() {
    state width = js("window.innerWidth")
    state later = "-"
    <div @mount={() => { later = `${js("window.innerWidth")}` }}>
        <p>width: {width}</p>          // "width: " on the page
        <p>in @mount: {later}</p>      // "in @mount: 1440"
    </div>
}
```

Repro: [Q-105-js-in-state-initializer-is-null.qwe](../repros/Q-105-js-in-state-initializer-is-null.qwe).

**Suggestion:** run a state initializer that calls `js()` again when the browser resumes the
component, so it means what it reads as. Failing that, `quillo check` should warn ("js() in a
state initializer gives null: the server renders this component; read it in @mount"), and the
reference example should read the theme in `@mount`.

### Q-102. A promise from `js()` is not waited for in a `let` function used as a handler

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the probe page stuck at "Trying…"; the bridge checked by hand; a browser repro | 5 |

In an inline handler (`@click={() => { let r = js(...) }}`), a promise from a page function is
awaited and `r` is its value. The same body written as `let go = () => { ... }` and used as
`@click={go}`, or called from `@mount`, gets the promise itself: `type_of(r)` is "promise".
A `server fn` called from such a `let` function *is* awaited, so the difference is only for
`js()`. Every call to the app's bridge (`quilloNative.location`, `.battery`, `.wait`) gives a
promise, so the probe page, written as `let run = () => { ... }`, read promises as values,
never finished, and never said why.

```quillo
component Waits() {
    state named = "-"
    let go = () => {
        let r = js("Promise.resolve", 5)
        named = `${type_of(r)} ${r}`        // "promise {}"; inline: "int 5"
    }
    <button @click={go}>named</button>
}
```

Repro: [Q-102-js-promise-in-let-handler.qwe](../repros/Q-102-js-promise-in-let-handler.qwe).

**Suggestion:** compile a component's `let` functions that call `js()` as async, as inline
handlers are, and await them where they are called from a handler or `@mount`. Workaround in
Fieldbook: every handler that waits for `js()` is written inline (the probe's whole run is the
button's handler, and `?run` presses the button).

### Q-103. A whole float from the browser reaches a `server fn` as an int, and a `float` parameter refuses it

| Kind | Detected by | Attempts |
|---|---|---|
| loud | saving a note: a 400 on the page | 1 |

JavaScript has one kind of number, so a reading that happens to be whole (the phone's location
accuracy was exactly 5 m) is sent as `5`, which the server reads as an int; `accuracy: float?`
then refuses it with a 400: "argument `accuracy` of `create_note` must be float?, got int".
It happens even for a literal `5.0` written in the component. Quillo never makes an int into a
float, which is right inside a program, but at this boundary the browser cannot say which it
meant, and a GPS accuracy, a heading of 90°, or a price of 12 fails some of the time.

```quillo
server fn half(x: float) = x / 2

component Halves() {
    state b = "-"
    <button @click={() => { b = str(half(5.0)) }}>halve</button>   // the call fails: 400
}
```

Repro: [Q-103-whole-float-from-browser-is-int.qwe](../repros/Q-103-whole-float-from-browser-is-int.qwe).

**Suggestion:** when a `server fn`'s arguments are made into their types (`server::take_args`),
take an int for a `float` (and in a `list[float]` or a record's float field), as a `dec`
already takes an int. The same holds for `Type(json)` of a JSON body. Workaround in Fieldbook:
`type Reading = float | int | null` for the note's numbers.

### Q-104. `quillo check` refuses `r.error` after `is_err(r)` for a `server fn` without `!`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

A `server fn` called from a component gives an error value when the call fails: no
connection, or an argument refused as in Q-103. The checker types the result as the declared
return (`float`), so `is_err(r) ? r.error : ...` is refused ("float has no field `error`").
The only way to show why a call failed is `str(r)`, or declaring `! str` on a function that
has no error of its own.

```quillo
server fn half(x: float) = x / 2
component Halves() {
    state said = "-"
    <button @click={() => {
        let r = half(5.0)
        said = is_err(r) ? r.error : str(r)   // check: float has no field `error`
    }}>{said}</button>
}
```

Repro: [Q-104-failed-server-call-error-refused.qwe](../repros/Q-104-failed-server-call-error-refused.qwe).

**Suggestion:** in a component, type a `server fn` call as its return or an error value, so
`is_err` narrows it.

### Q-100. `quillo check` passes `js(name)` with the name in a variable, which loading refuses

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo run` (the program did not start) | 2 |

The probe first walked a list of names (`for api in apis { js(api[1]) }`). `quillo check` said
ok. Starting the program failed: "`js` needs the name of what it calls written out". The rule
is right and documented; only the checker does not know it.

Repro: [Q-100-check-misses-js-name-in-a-variable.qwe](../repros/Q-100-check-misses-js-name-in-a-variable.qwe).

**Suggestion:** give the checker the same rule, with the same message.

### Q-101. A load error from compiling a component for the browser names main.qwe and another file's line

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the load error of Q-100 | 1 |

The error of Q-100 said `main.qwe:87`, in a main.qwe of 32 lines; line 87 was in
ui/probe.qwe. With several files, the line points nowhere.

```
quillo: main.qwe:9: <Probe> (compiled for the browser): `js` needs the name of what it calls written out, like js("Chart", ...)
```

Repro: [Q-101-load-error-names-main-file/](../repros/Q-101-load-error-names-main-file) (the line is ui/probe.qwe:9).

**Suggestion:** name the component's own file, as run-time errors do since Q-016.

### Q-107. Reading a member a module does not have gives null, and `quillo check` says nothing

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a 500 from a sample route, then a two-line program | 1 |

`json.title` and `db.nothing` read as `null`, at run time and in the checker. It came up
writing `route POST /notes { json: { title: str, body: str } } = create_note(json.title,
json.body)`: an inline shape binds `title` and `body`, not `json`, so `json` is still the
module, and the route failed later with "argument `t` of `create_note` must be str, got
null". A call of a missing function is caught; reading one is not.

```quillo
print(json.title)   // null; check: ok
print(db.nothing)   // null; check: ok
```

Repro: [Q-107-missing-module-member-is-null.qwe](../repros/Q-107-missing-module-member-is-null.qwe).

**Suggestion:** a checker error, "`json` has no `title`", and for this route's case a hint
that the shape binds its fields.

### Q-106. Quillo does not build for Windows on ARM

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | building in a Windows 11 ARM64 virtual machine | 1 |

`corosensei`, which `src/httpd.rs` and `src/stack.rs` use for coroutines, stops with
"Unsupported target" on `aarch64-pc-windows-msvc`. CI builds Windows only for x64, so this
never showed. The x64 build ran on Windows 11 ARM under emulation: `quillo.exe` starts, and
`quillo app` opens its native window, serves the page (200 for the page and its script) and
takes the page's title. In the screenshots taken inside the VM the page area stayed white.
Whether that is the VM's display, emulation, WebView2 or the screenshot is not yet known, so
the desktop window on Windows is not proven.

**Suggestion:** Windows fibers behind the same interface where `corosensei` has no port, and
an ARM64 Windows job in CI.

## A real iPhone

The simulator's results below stand, but a simulator has no sensors, mock cameras and no
person to tap. So Fieldbook was then built for a device and run by hand on an iPhone 17 Pro
Max (iOS 26.7.1), signed with a free Apple ID's Personal Team. Nothing in the program
changed, and no Quillo core change was needed.

The probe, run with a real tap: **37 checks, 26 work, 0 need a tap, 1 refused, 10 not there,
0 errors.** Two notes were saved by hand. The first had the place from the app (±5.7 m), a
compass heading and a 3-second voice memo. The second had a place, a 2.3 MB photo from the
camera, a 26 KB thumbnail made on the phone by `image.convert`, and a memo. Both fetched
their weather, and the minute job ran.

| What | iOS simulator | Real iPhone |
|---|---|---|
| Location, the app | a position set by `simctl`, ±5 m | real, ±6 m |
| Location, the web view | a second prompt that could not be answered | real, ±6 m, once allowed |
| Compass heading | none | real |
| Camera | "Mock video device 1" | "Front Ultra Wide Camera" and the phone's others |
| Microphone | "Mock audio device 1" | "iPhone Microphone" |
| Recording | audio/mp4 | audio/mp4, played back |
| Orientation, motion permission | "requires a user gesture" | granted |
| Motion events, 1.5 s | 0 | 88 |
| Battery, the app | level not said, unknown | 80%, charging |
| `navigator.wakeLock` | "Permission was denied" | held, then released |
| Clipboard write | refused | written |
| Clipboard read | refused | refused |
| Notifications, the app | the prompt could not be answered | scheduled |

Three things the simulator said were limits of the simulator, not of iOS: the wake lock,
writing to the clipboard, and everything behind a tap. What is missing from WKWebView is the
same on both: Vibration, Battery, Network Information, full screen, Bluetooth, NFC, USB,
Serial and HID. `speechSynthesis` is there, with 0 voices listed on both.

Three things are not yet tried on the phone by anyone: a notification seen arriving, the
app left for hours, and force-quitting and reopening.

## The phone's hardware, tried in the simulator

Each row is what the probe page found, in the app's web view (WKWebView) on the iOS 27
simulator, with location, camera, microphone, photos and motion granted by `simctl privacy`.
"the app" rows go through `window.quilloNative`; the others are the web platform's own.

| What | How | Answer | Detail |
|---|---|---|---|
| Location | the app (CoreLocation) | yes | 1.2834, 103.8607 ±5 m (`simctl location set`) |
| Location | the web view (`navigator.geolocation`) | asks | a second prompt, "127.0.0.1 would like to use your current location", though the app has the permission |
| Camera | getUserMedia video | yes | "Mock video device 1" (the simulator has no camera) |
| Microphone | getUserMedia audio | yes | "Mock audio device 1" |
| Recording | MediaRecorder | yes | audio/mp4; codecs=mp4a.40.2, 2.5 s is about 9.5 KB |
| Media devices | enumerateDevices | yes | 4 audio inputs, 2 video inputs, 4 audio outputs |
| Orientation, motion | DeviceOrientationEvent / DeviceMotionEvent.requestPermission | asks | "requires a user gesture": only a real tap |
| Motion events | devicemotion for 1.5 s | no | 0 events (no permission without a tap; none on a simulator anyway) |
| Vibration | navigator.vibrate | no | not in WebKit |
| Haptics | the app (UIFeedbackGenerator) | yes | success |
| Battery | navigator.getBattery | no | not in WebKit |
| Battery | the app (UIDevice) | yes | level not said, state "unknown" (a simulator has no battery) |
| Screen stays on | navigator.wakeLock.request | refused | "Permission was denied" in the web view |
| Screen stays on | the app (idle timer) | yes | set |
| Brightness | the app (UIScreen) | yes | 50% |
| Online | navigator.onLine | yes | true |
| Connection type | navigator.connection | no | not in WebKit |
| The internet | the program's server (`http.get`, TLS) | yes | 200, 28.9 °C in Singapore |
| Clipboard | navigator.clipboard.writeText / readText | refused | without a tap, "not allowed by the user agent" |
| Share sheet | navigator.share | yes | can share text |
| Share sheet | the app (UIActivityViewController) | yes | opens with the note's text (screenshot) |
| Speech | speechSynthesis | yes | 0 voices on the simulator |
| Dictation | webkitSpeechRecognition | yes | present |
| Notifications | the web's Notification | no | not in a web view |
| Notifications | the app (UNUserNotificationCenter) | asks | the system prompt; `simctl privacy` cannot grant notifications and nothing can tap it (see Limitations) |
| Local storage, IndexedDB | | yes | |
| Storage | navigator.storage.estimate | yes | 9.6 GB quota |
| Screen orientation | screen.orientation | yes | portrait-primary |
| Full screen | document.fullscreenEnabled | no | |
| Bluetooth, NFC, USB, Serial, HID | navigator.bluetooth, NDEFReader, navigator.usb/serial/hid | no | none is in WebKit; each needs a native bridge |
| Photo | `<input type=file accept=image/* capture>` | yes | a 1600×1200 JPEG given to the input, saved, and a 480×360 thumbnail made by `image.convert` on the phone |
| The minute job | `schedule("1m", ...)` | yes | ran each minute while the app was open; none while it was in the background; one run on return for the minutes missed, then each minute again |

What `platform()` said on the phone: os `ios`, os_name `iOS`, os_version `27.0`, device
`phone`, model `iPhone19,2`, model_family `iPhone`, simulator `true`, runtime `mobile`, arch
`aarch64`, 10 cpus, memory 17,179,869,184, locale `en-SG`, time zone `Asia/Singapore`, screen
402×874 at 3×, and the app's name, id, version and build.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| A list of `[name, path]` and `js(api[1])` in a loop | less code for 35 probes | `quillo check` ok, the program did not start (Q-100), and the error named main.qwe:87 (Q-101) | 2 runs |
| `js("quilloWait", 1500)` to let motion events arrive | a component has no sleep, and `setTimeout` is refused | not defined: I added `quilloNative.wait` to the app's bridge | 1 run |
| `let run = () => { ... }` with `@click={run}` | a long handler reads better as a named function | the page stuck at "Trying…" (Q-102); I checked each bridge call by `--eval`, then wrote the repro | 5 runs |
| `js("navigator.bluetooth") == null ? "no" : "yes"` | JavaScript's `undefined` check | a missing name is an error value (Q-054), not null: Bluetooth, USB, Serial and HID said "yes" | 1 run |
| `accuracy: float?` and the other numbers of a note | they are floats | the save failed on a whole 5 m (Q-103) | 1 run |
| `is_err(r) ? r.error` after a `server fn` call | the guide's error-value pattern | `quillo check` refused it (Q-104) | 1 check |
| `let thumb = image.convert(...)` kept as the thumbnail's path | read the name, not the reference | it is `{ width, height }`: the list showed no thumbnail | 1 run |
| `<article.note>` for the note page | a natural name | `.note` was my warning style: the whole page in orange | 1 screenshot |
| `state battery = js("quilloNative.battery")` | the reference's example reads localStorage this way | always null (Q-105) | 2 screenshots, 1 repro |
| A three-column table of probe results | as on a desktop | on a phone the detail column was one letter wide; rows became a grid | 1 screenshot |
| Saving the probe's rows after the last step | the end of the run | the last step (notifications) waits for a tap that never came, so nothing was saved; rows are now kept first | 2 runs |
| Effects, a `server fn` in markup, `state_events` | my slips | each caught by `quillo check` | 3 checks |
| `json.title` after an inline `{ json: { ... } }` shape | as with `{ json: Order }`, which binds `json` | null at run time, nothing from the checker (Q-107) | 1 run |
| A simulator's `wakeLock` and clipboard refusals, written up as iOS's | they were what the probe found | a real phone does both; the guide and this report said otherwise until corrected | 1 phone |
| Running `./gradlew` by hand after build.sh | quicker | no app.toml read, so the APK took the default id and I tested the old install | 3 runs |

## Limitations

- **No pause in a component.** There is no `sleep` in the browser and `setTimeout` is refused by
  `js()`, so waiting 1.5 s for sensor events needs a page script. Fieldbook uses the app's
  `quilloNative.wait(ms)`; a browser program would need its own script.
- **No way to ask whether a page function exists without calling it.** `js("x.f")` with no
  arguments calls `f` when it is a function. `js_value` reads without calling, but only
  through a library's `use` name. A probe for `navigator.share` has to call `canShare` instead.
- **What needs a tap cannot be scripted.** Orientation and motion permission, the clipboard,
  and anything else WebKit keeps for a user gesture refuse a click from `--eval` or `?run`.
- **The simulator's prompts.** `simctl privacy` grants location, camera, microphone, photos and
  motion, but not notifications. This Xcode has no Simulator window app, so nothing can tap
  "Allow"; the prompt stays over the app until the simulator is rebooted.
- **The web view's geolocation asks again**, under the name "127.0.0.1", though the app holds
  the location permission. Programs in the app should use `quilloNative.location`.
- **A free Apple ID's signing lasts 7 days**, and a device build needs a small Xcode project
  once, to make the certificate and profile (mobile/README.md).
- **Web APIs WebKit does not have** (Bluetooth, NFC, USB, Serial, HID, Vibration, Battery,
  Network Information) need native calls in the app's bridge to be reached at all.
- **No work in the background.** iOS suspends the app, and the program's server with it, soon
  after it leaves the screen. A `schedule` job did not run during 150 s behind the Settings
  app; on return it ran once for the three minutes missed, then each minute again:

  | When | Runs of the minute job | Last run |
  |---|---|---|
  | start | 17 | 06:43 |
  | 70 s in the foreground | 18 | 06:44 |
  | 150 s in the background | 18 | 06:44 |
  | 70 s after returning | 20 | 06:48 |

  Work that must happen while the app is away needs the system's own means (background
  tasks, push), which the runtime does not offer yet.

## Fixed on the way (branch `mobile`)

- The iOS app's `--eval` takes several scripts, run in turn, one per page loaded, so one can
  go to a page and the next act on it (`mobile/ios/Quillo/App.swift`).
- The launcher sends the device's facts with sorted keys: a Swift dictionary's order changed
  from launch to launch, and so did the order of `platform().app` and `.screen`.
- The bridge has `quilloNative.wait(ms)` (at most 60 s).

## Edge cases checked that behave well

| Case | Result |
|---|---|
| A note with no place (`place` null, `place.lat` passed on) | saved with null coordinates; the note page says "This note has no place" for the weather |
| A photo and a memo given to `server fn`s as files from a component | sent as multipart, saved under `media/` in the app's container |
| `image.convert` of a 1600×1200 JPEG to 480 px | 480×360 written on the phone |
| The memo served back from the program's own route | the note's `<audio>` element loads it and shows its 2 s |
| `http.get` with TLS from the phone's program, cached in the database | the weather on the note, fetched once |
| The program's database across reinstalls of the app | kept: the code follows the bundle, the `.db` stays |
| A promise from the bridge in an inline handler | awaited; with location revoked, the refusal is an error value the page shows: "js: location is turned off for this app" |
| `live(load_notes)` after saving notes | the list shows both notes, with the thumbnail and "memo" |
| The probe opened with `?run` | `@mount` presses the button, so the run is the tapped handler |
| A system prompt over the page while the program serves | the program keeps serving; only taps are blocked |
