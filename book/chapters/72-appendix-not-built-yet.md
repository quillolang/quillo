# Appendix B: Not built yet {#not-built-yet}

- `match` exhaustiveness in the checker
- compiling components to WebAssembly (they compile to JavaScript)
- a formatter
- a file's names under a prefix (`use lib/auth as auth`, then `auth.login()`), where `.`
  in markup is already a class
- splitting a page's bundle by route, and components loaded lazily
- a caller finding a service by name through limer's pool, and [`--join`](#options-of-quillo-run) for copies behind NAT
- `{{ ... }}` blocks of JavaScript in a component (a design only)
- in the browser, a map's keys that look like integers keep their order of insertion:
  there `keys({ "10": 1, "2": 2 })` is `["2", "10"]`, as JavaScript orders them
- Linux builds linked with musl, or static: not planned, since a static binary cannot
  load plugins. The releases from the next tag on are linked against glibc 2.17 instead
- a catalog whose entries are signed, its plugins prebuilt for Linux, and `//! effects:`
  on each of them; a policy that allows catalog plugins only (`local = false`)
- `quillo run --sandbox` (seccomp and Landlock on Linux), and plugins built to
  WebAssembly and run sandboxed
- a policy's `private = false` for `db.connect` and UDP (it covers `http.*` only)
- Passenger (cPanel's Application Manager) starting a Quillo program
- phone apps in the stores: uploads to the App Store and Google Play, HarmonyOS NEXT,
  `.qwer` plugins linked into the app, and work while the app is in the background
- `quillo app` without a socket run on Windows and Linux (the code is wry's on all three;
  only macOS has run), and a `stream` route's answer sent in parts to an app's web view
- `quillo build` making a program's `build` declarations ahead of time
- an HTTP/2 client, for APNs and gRPC
