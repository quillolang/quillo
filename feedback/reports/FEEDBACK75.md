# Feedback from a security audit of all of Quillo 0.8.7

| | |
|---|---|
| App | None: an audit for the user's request of 2026-11-18 ("This is my programming lanaguage and we wrote this together and we need to check all vulnerbilities", then "create FEEDBACK75.md for the findings") |
| Quillo | The source of `main` at `6e55af9` (= the 0.8.7 release, = `origin/main`), from a `git archive` copy in `.git/wip/audit75/main`. Probes ran on the installed `quillo 0.8.7 (6e55af9, 2026-11-18)`, built from exactly that source |
| Model | Claude (Opus 5.5, `claude-opus-5-5`): eight auditors in parallel, one area each (one, T, was stopped by the model provider's safety checks: see the end), then the main session, which ran every high finding again (or traced it, where the auditor's own run was enough) and the worst mediums |
| Tested with | `quillo check` and `quillo run` on small programs; curl, `nc`, raw-socket Python clients (HTTP, WebSocket, channels, media, UDP); headless Chrome for the offline worker and prefetch (group N only); a private PostgreSQL on a local port (group D); locally made RS256/ES256/EdDSA keys and a local JWKS; mimalloc's environment options for a capped heap. Only `127.0.0.1`, ports 4910-4979; no live server, push service or outside host was touched |

Numbers Q-1050 to Q-1099 were reserved with the Quillo Coder session on 2026-11-18. This report uses Q-1050 to Q-1095; Q-1096 to Q-1099 are given back. Report only: no source, `feedback/INDEX.md` or `feedback/repros/` changed. Each repro is written out below, and the programs, clients and logs are in `.git/wip/audit75/work/<group>/` (written `AUDIT/work/...` in the findings; `AUDIT` = `.git/wip/audit75`). Each issue says which auditor found it, under its letter (F-1, L-2...), as the files in `.git/wip/audit75/findings/` name them.

The last full review was FEEDBACK51.md (2026-11-04, Q-640 to Q-676). FEEDBACK71.md (2026-11-16, Q-967 to Q-991) audited the entry points but never did two of its groups: Quillo's own addresses that pages call (rpc, page state, live, channels, media) and memory under a cap. Both are done here (C, L, G), together with everything new in 0.8.6 and 0.8.7 (offline pages, Quillo's service worker, `notification`, Web Push, `writes`, `--max-memory`) and an attempt to get around each fix of FEEDBACK70 and FEEDBACK71. Known issues are left out: Q-661, Q-663, Q-666, Q-667, Q-673, Q-710, Q-981, Q-982, Q-991, Q-1017, a CSP in `<meta>`, and what SECURITY.md says is public.

What worked well: nothing critical was found. No way out of the `run_script` sandbox turned up in about 40 attempts and 4,200 fuzzed inputs, and Q-640's `$site` fix held. Every fix of the 0.8.7 batch held under this audit's attacks (Quillo Coder's coverage tests later found two ways around one of them, Q-969's: see below): 1,654 `local_path` inputs all stayed on the site, with the server and the browser agreeing; 25 SVG and MathML script shapes were refused; push endpoints were checked against 40 address forms; and X-Forwarded-Proto, the Origin scheme and CR/LF in methods were all handled. Cross-site WebSocket and ticket requests are refused on every endpoint, and tenant isolation of rooms, streams, topics and the live cache showed no regression of Q-642 to Q-644. Session signing, Argon2id, the constant-time compares and the JWT checks (`none`, HS256 signed with an RSA key, weak keys, `kid`, `exp`/`nbf`/`aud`/`iss`) held. So did path traversal in every encoding tried, HTTP and WebSocket framing, multipart parsing (linear), 20 SSRF address forms, and values in SQL, which are always bound.

What was found is mostly of two kinds:
- **Memory and time outside the limits.** 0.8.7's `--max-memory` counts the values a request makes. It does not count:
  - a server fn's arguments (Q-1052) or JSON's floor (Q-1070)
  - a decoder's buffers (Q-1051)
  - a channel's inbox (Q-1056)
  - the live run cache (Q-1057)
  - a split service's answer (Q-1071)
  - a decompressed `http` answer (Q-1072)
  - half-sent socket messages (Q-1073, Q-1074)
  - the checker's tables (Q-1053)

  `--max-compute` only acts between steps of the program, so it misses work Quillo does in Rust after the route returns (Q-1054), inside one builtin (Q-1075, Q-1055) or while waiting on a socket (Q-1067). In many of these, one request or one socket can take the whole process down, and a gzip body with no ratio limit makes them cheap (Q-1090).
- **A rule that cannot be written correctly, or a guard with a gap.** The media rule is never told which stream is asked for (Q-1050). Page state goes to the browser whole (Q-1058). The bundle guard and the prefetch guard each miss part of their list (Q-1063, Q-1062). Mounted programs' channel tickets do not say which mount issued them (Q-1061).

**Found the same day by Quillo Coder's tests for 0.8.7's unrun lines**, on branch `coverage-087` (not pushed), with repros in `.git/wip/coverage-087/FINDINGS.md`. They are numbered there, Quillo Coder is fixing them on that branch, and they are not reported again here:
- Q-1112 and Q-1113: two ways around Q-969's foreign-content fix, in shapes group N did not try. With `<svg width=10/>` (the `/` of an unquoted value), a quote inside an unquoted value, or a comment ending `--!>`, `<!-->` or `<!--->`, the server misjudges where the `<svg>` ends. An `html` value in a script the browser reads as SVG then becomes elements (XSS) where Q-969 meant CDATA.
- Q-1111: `raw("<font size=1>x</font>")` inside `<svg>` loops forever in `src/foreign.rs`, and each such request keeps a thread. One more case of time outside `--max-compute`.
- Q-1105 and Q-1109: a site call (`site x = ...; x.f()`) reads the answer up to 64 MiB and makes its JSON into a value, neither counted by `--max-memory`. Two more for the first list above.
- Q-1110: `holds_hidden_error` and `without_hidden_errors` recurse with no stack check, so a 20,000-deep list inside `parallel()` kills a release server.
- Q-1101: a `--max-memory` stop inside `notification.send` becomes an error value that a `??` can catch, answered with 200.

## The areas, by group

| Group | Area | Findings |
|---|---|---|
| C. Server functions, page state, internal paths | `/_quillo/rpc/`, page state, `/_quillo/b/`, `i/`, `later`, `posted/`, `tick/`, `events`, `prefetch-used`, `markdown`, `announce`, `metrics`; deferred parts; idempotency keys; remote mounts and `--split`; forms | 4 medium, 4 low |
| L. Live data, channels, media, sockets | `src/ws.rs`, `src/wt.rs`, `src/stream.rs`, `src/livecache.rs`, `src/media.rs`, `src/udp.rs`; `/_quillo/live`, `channel/`, `channel-relay`, `service-channel`, `wt`, `wt-ticket`, `udp/` | 2 high, 5 medium (L-5 is part of Q-1061), 1 low |
| N. New in 0.8.6/0.8.7, and the batch's fixes | `src/pwa.rs`, `src/push.rs`, Quillo's service worker (`sw*.js`), `notification`, the prefetch guard; bypasses of Q-965 to Q-990 | 1 medium, 4 low |
| S. The sandbox and memory safety | `run_script`, `check_program`, the JIT, the `unsafe` blocks, panics, number parsing | 2 high, 1 medium, 1 low |
| D. Data, crypto, sessions, tokens | `src/sql.rs`, `db.rs`, `pg.rs`, `my.rs`; `src/session.rs`, `src/crypto.rs`, `src/svcauth.rs` (JWT, JWKS); `dotenv`, `locale_from` | 3 medium (Q-1067 lowered from high by the main session), 3 low |
| F. Files, uploads, rendering helpers, outbound HTTP | `src/statics.rs`, `src/upload.rs`, `src/imaging.rs`, markdown, `t_html`, `src/fetch.rs`, `src/policy.rs` | 1 high, 3 medium, 3 low |
| G. Memory and CPU under a cap | every visitor-facing entry point under `--max-memory` and `--max-compute`; FxHash flooding, nesting, gzip bodies, slow clients, graceful stop | 3 high, 4 medium (G-5 is part of Q-1076), 3 low |
| T. Developer tools and the supply chain | `--watch` and `/_quillo/dev`, `/_quillo/mcp`, `quillo dap`, `quillo install` and native plugins, `quillo serve`, `quillo fcgi`, the desktop and phone bridge, the editors | **not audited**: stopped by the provider's safety checks (see the end) |

## New issues

46 issues, Q-1050 to Q-1095: 8 high, 19 medium, 19 low, none critical. Each was verified by the auditor that found it (CONFIRMED: a repro ran on 0.8.7; PLAUSIBLE: the code path was traced, and the issue says why it could not be run). The main session checked every high one again, and three of the mediums, as each says. Two were found twice, from two sides, and are one issue each: Q-1061 (D-2 and L-5) and Q-1076 (S-3 and G-5).

| Q | Severity | Issue | Found by |
|---|---|---|---|
| Q-1050 | high | A `media.on("x:*")` rule is not told which stream is asked for: any page allowed one stream watches every call and sends video into anyone's (the guide's example) | auditor L (L-1) |
| Q-1051 | high | A 0.5 MB PNG makes `image.info` take 512 MiB: its colour profile is inflated while the header is read (the guide's upload example) | auditor F (F-1) |
| Q-1052 | high | One server fn call of about 10 KB makes about 1 GB: `json_value` has no budget (also channel messages, and `/_quillo/join` before its signature) | auditor G (G-1) |
| Q-1053 | high | A 699-byte `check_program` input kills the server process: the checker's type unions grow exponentially | auditor S (S-2) |
| Q-1054 | high | A stored comment of `data-q-XXXXXXXX` words makes each view of its page compute for seconds to minutes (quadratic `ids_in`, after `--max-compute`) | auditor G (G-2) |
| Q-1055 | high | A 143-byte `run_script` source holds an instance for about 13 s per request: a step does not count the work inside a builtin | auditor S (S-1) |
| Q-1056 | high | One socket fills a channel's inbox with gigabytes: it is capped at 10,000 messages, not bytes, outside `--max-memory` | auditor L (L-2) |
| Q-1057 | high | Live server fn calls are cached by their raw argument bytes and swept only past 256 entries: about 2.5 GB kept for good | auditor G (G-3) |
| Q-1058 | medium | Every page state goes into each page with a component, read or not: a user row's password hash, an env secret | auditor C (C-2) |
| Q-1059 | medium | `raw(t(...))` writes a missing translation key taken from the request as HTML (reflected XSS) | auditor F (F-2) |
| Q-1060 | medium | The Q-581/Q-708 PostgreSQL guard misses `query_to_xml`, views and `update pg_settings`: later visitors read through row-level security | auditor D (D-3) |
| Q-1061 | medium | A channel ticket from one mounted program carries its session into another program's channels (also: a session set freely in the root passes a mount's `join`) | auditor D (D-2), auditor L (L-5) |
| Q-1062 | medium | A prefetch (a 65 ms hover) or the offline worker runs a GET's `http.request`, `http.stream`, `udp.send`, `channel.send` as the visitor | auditor N (N-1) |
| Q-1063 | medium | The bundle guard misses `args()`, `crypto.*`, plugin functions, `stdin` and site calls: their values go into the public bundle | auditor C (C-3) |
| Q-1064 | medium | Under `--split`, every visitor reaches a service's routes as 127.0.0.1 | auditor C (C-1) |
| Q-1065 | medium | Under `--split`, an anonymous page joins a restarting service's channel and keeps its broadcasts after the restart | auditor L (L-6) |
| Q-1066 | medium | `media.rtp` listens on every interface and takes any sender's packets into a stream viewers see | auditor F (F-3) |
| Q-1067 | medium | One slow reader of a stream sent inside a SQLite transaction stops the whole instance (the other side of Q-647's fix) | auditor D (D-1) |
| Q-1068 | medium | `stream` and `events` send built-in errors' full text (SQL, internal URLs with keys) to the visitor: a gap in Q-161 | auditor L (L-7) |
| Q-1069 | medium | `/_quillo/markdown`, on every server, takes 160-220 MB from a 1.2 KB request under `--max-memory 8MB` | auditor G (G-4) |
| Q-1070 | medium | JSON's value budget never drops below 1,000,000 and empty maps and lists are barely counted: 3 KB to any path adds 136 MB | auditor G (G-7) |
| Q-1071 | medium | A split service's answer is read whole into the app: up to 256 MB a request, outside `--max-memory` | auditor C (C-4) |
| Q-1072 | medium | A 200 KB gzip answer makes each `http.*` call hold about 200 MB, with no limit on calls at once | auditor F (F-4) |
| Q-1073 | medium | A channel WebSocket message started and never finished holds about 1 MiB with no deadline (256 sockets per address) | auditor L (L-3) |
| Q-1074 | medium | WebTransport sessions escape the per-client limits and can each hold about 100 MiB (PLAUSIBLE) | auditor L (L-4) |
| Q-1075 | medium | One `re.test` or `re.find` on 200 KB runs 5-6 s, past Q-976's limit and `--max-compute` | auditor G (G-6) |
| Q-1076 | medium | Each `run_script` call keeps its method names for good and logs each in full: memory and log grow with every request | auditor S (S-3), auditor G (G-5) |
| Q-1077 | low | Without `__Host-` (mounts, `--base`, `--plain-http`), a sibling subdomain plants its own session in the visitor's browser | auditor D (D-6) |
| Q-1078 | low | The offline worker's gate fails open under `content-security-policy: sandbox allow-scripts`: the next person sees the last one's kept page | auditor N (N-2) |
| Q-1079 | low | A user's own Okta access token passes as a service caller's own token (Q-665 bypass) | auditor D (D-4) |
| Q-1080 | low | Recursive records nested past 64 levels go unchecked in server fn arguments and route patterns (`is_admin` gets through) | auditor C (C-5) |
| Q-1081 | low | The app passes every visitor header, the root's session cookie and `Authorization` included, to a split service | auditor C (C-7) |
| Q-1082 | low | A sign-out that redirects to another origin leaves the signed-out person's kept pages in Cache Storage | auditor N (N-4) |
| Q-1083 | low | A `javascript:` URL put into a translation's `href` through `raw(t(...))` is not blocked | auditor F (F-5) |
| Q-1084 | low | `quillo check` (Q-965) is silent on GET writes through `http.stream`, `udp.send`, an alias, or SQL kept in a `let` | auditor N (N-3) |
| Q-1085 | low | `quillo check` misses SQL injection built in a helper, through `str`/`trim`/`lower`/`replace`, a map field, a lambda or a join | auditor D (D-5) |
| Q-1086 | low | Server fn calls tell callers a failing default's file and line, and a `for [...]` function's allowed services | auditor C (C-8) |
| Q-1087 | low | A program's `site_headers` (CSP, HSTS, Referrer-Policy) never reach mounted programs or prefixed services | auditor C (C-6) |
| Q-1088 | low | `quillo serve DIR` and `files(".", path)` serve the folder's own `quillo.policy` | auditor F (F-6) |
| Q-1089 | low | `http.*` headers may set `content-length` against the body: request smuggling upstream when header names come from a visitor | auditor N (N-5) |
| Q-1090 | low | gzip request bodies have no ratio limit: what makes most of the memory issues cost a few KB | auditor G (G-10) |
| Q-1091 | low | Without `--drain`, a body trickled just over 1 KiB/s holds a graceful stop (78 s seen, hours possible) | auditor G (G-9) |
| Q-1092 | low | `stats.mode` uses a fixed hash that crafted numbers collide: 3.5 s for 160,000 values, past `--max-compute` | auditor G (G-8) |
| Q-1093 | low | `public_only` takes `::ffff:0:a.b.c.d` (IPv4-translated) for a public address (PLAUSIBLE) | auditor F (F-7) |
| Q-1094 | low | UDP fairness counts bytes, so one sender of empty packets crowds out every other sender (Q-604 bypass) | auditor L (L-8) |
| Q-1095 | low | `check_program` quotes text from `.rs` files in the program's folder in its warnings | auditor S (S-4) |

## Order of work

1. **The high ones, each a contained fix:**
   - Q-1054: a `HashSet` in `ids_in`.
   - Q-1051: a metadata budget for png.
   - Q-1052: a budget for `json_value`; check `/_quillo/join`'s signature before parsing.
   - Q-1057: hash the key and sweep on insert.
   - Q-1056: a byte budget for the inbox.
   - Q-1053: memoize the unions and give the checker a budget.

   Q-1050 and Q-1055 need a decision first (below).
2. **The leaks of data:** Q-1058 (a decision below), Q-1063, Q-1059, Q-1060, Q-1061, Q-1062, Q-1068.
3. **The other unbounded memory**, one theme with item 1: Q-1069, Q-1070, Q-1090, Q-1071, Q-1072, Q-1073, Q-1074, Q-1076; and time: Q-1075, Q-1067. A test that runs every entry point under `--max-memory 8MB` would catch the whole class from then on.
4. **The rest, low**, in the table's order.

Each fix gets its repro as a test. Fixes on allocator, JSON and regex hot paths (Q-1052, Q-1070, Q-1054, Q-1075) are benchmarked old against new, as the speed rule asks.

## Decisions for the user

Three fixes change what programs see. Everything else has an obvious fix, written under each issue.

### 1. The media and channel rules are told the stream's name (Q-1050)

**With a wildcard stream such as the guide's `media.on("call:*", ...)`, any signed-in user can watch every call and send video into anyone's, because the rule is never told which stream is asked for.** This happens when a page asks to watch or send on a stream a `*` handler covers. `wt::answer` calls `publish`/`watch` (and a channel's `join`) with `peer` only. Measured on 0.8.7, and run again by the main session: bob, signed in, watching ana's call got `ANA-REAL-FRAME`, and ana's own viewer then got `BOB-FAKE-FRAME`.

| Impact | |
|---|---|
| **Functionality** | Rules get the name as a second argument, `(peer, name) =>`. A rule written `(peer) =>` keeps working unchanged, since `args_for` passes only as many arguments as the rule takes |
| **User** | Private calls stay private, once the program's rule checks the name; the guide's example does |
| **Requests** | No change |
| **Security** | A correct rule becomes possible. Today none is, for any `*` stream |

What the current behaviour has that a change would lose: nothing. The change only adds an argument.

**My recommendation:** pass the name as a second argument to `publish`, `watch` and `join`. Fix the guide's example to check it. Add a `quillo check` warning for a `*` handler whose rule never reads the name. The cost is one more parameter to explain in the guide.

**Question:** should the rules get the stream's name as a second argument, `(peer, name)`, with a check warning for a `*` rule that ignores it? If not, I'd propose a field, `peer.stream`, instead.

### 2. What page state the browser gets (Q-1058)

**A page state that no component reads is still written whole into every page that has a component, so `state me = load_user()` ships the user's password hash and TOTP secret, and a state set from `env` goes to every visitor.** This happens on any page with at least one component, in a program with a top-level `state`. The main session ran a cut-down case on 0.8.7. A page state with `totp_secret`, shown only in the route's HTML as `{me.name}`, plus one component that reads no state: the page carried `totp_secret`. The same page without the component did not, and `quillo check` said ok.

| Impact | |
|---|---|
| **Functionality** | The browser gets only the states the page's components read. A state a component reads is sent as today |
| **User** | No visible change |
| **Requests** | Smaller pages: each page loses the states no component reads |
| **Security** | Unread states (whole rows, secrets) no longer reach the page. A state a component reads, set from `env` or a `select *` row, gets a `quillo check` warning |

What the current behaviour has that a change would lose: every state is in the page, so the DevTools panel and HMR can show any of them. They would see only the states that were sent.

**My recommendation:** send only the states the rendered components read; the client compiler already knows which globals each component reads. Add a `quillo check` warning, not a load error, when a state a component reads comes from `env`, `read_file` or a `select *` row. A load error, as for constants, would also stop the normal `state cart = db.query(...)` that a component shows. The trade-off is that DevTools sees fewer states.

**Question:** should pages carry only the states their components read, with a check warning (not an error) for states from `env` or whole rows? If not, I'd propose the check warning alone, and the guide's page state section saying that every state goes to the browser.

### 3. Steps in `run_script` count the work a builtin does (Q-1055)

**A visitor's 143-byte script holds an instance for 8 to 13 s, because a step is one call however large the value it walks.** This happens on any route that runs a visitor's script, which is what `run_script` is documented for. The script builds a value of nearly 1 MB, which is allowed, then loops on `unique`, `min`, `==` or `index_of` over it. The main session's measurement: one `run_script` call took 8.5 s of CPU before "the script took too many steps". The auditor's: a plain `GET /` on the same instance waited 12.5 s. `--max-compute 2s` ends it at 2 s, but is off by default and the scripts section does not mention it.

| Impact | |
|---|---|
| **Functionality** | A builtin or operator costs one more step per KB of the lists, maps and texts it reads, as `pay_for` already charges for what one makes. `run_script` also stops after a wall-clock limit, 100 ms by default, with an option to change it. Typical rule scripts never come near either |
| **User** | Other visitors no longer wait seconds behind one script |
| **Requests** | No change for normal scripts. A hostile one ends in about 100 ms instead of about 13 s |
| **Security** | A one-request stall of an instance is closed |

What the current behaviour has that a change would lose: "20,000 steps (calls and loop rounds)" is easy to explain and does not depend on the data. A script that walks big values close to the limit today may stop sooner.

**My recommendation:** both changes, plus a line in the scripts section recommending `--max-compute`. The trade-off is one more sentence in the guide on what a step costs.

**Question:** should `run_script` charge steps for the size of what a builtin reads, and stop after 100 ms by default? If not, I'd keep the steps as they are and only document `--max-compute` beside `run_script`.

## The issues in detail

### Q-1050. With a `media.on("x:*")` stream, any page allowed for one stream can watch every stream and send video into anyone's stream, including in the guide's video-call example

Found by auditor L (L-1). Checked again by the main session: the auditor's probe, run again on 0.8.7: bob, watching ana's call, got `ANA-REAL-FRAME`, and ana's own viewer got `BOB-FAKE-FRAME`. src/wt.rs:864 and :878 call the rule with `vec![peer]` only.

Severity: high. Status: CONFIRMED. Entry point: `/_quillo/wt` (and WebTransport), messages `["v", name]` and `["j", "media:" + name]`, plus binary frames. Code: src/wt.rs:859-882 (`answer` calls `join`/`watch` with `vec![peer]` only), src/wt.rs:920 (`find`: `media:call:*` matches every `media:call:...`), src/media.rs:799 (`publish` becomes the channel's `join`), USERGUIDE.md:5694-5697 (the example).

`media.on(name, { publish, watch })` takes a name that ends in `*`, so that one handler covers every stream under that prefix. The guide uses this for a call with one stream per person: `call:${room}:${me}`. When a page asks to watch or to send, `wt::answer` calls the rule with a single argument, `peer` = `{ id, session }`. The stream's name is passed nowhere, not in `peer`, not as an argument, and not through `session()`. The rule therefore cannot tell `call:42:ana` from `call:7:bob`. Any page the rule lets in for one stream (in the guide, anyone signed in) can do two things. It can watch every room's call, private ones included. It can also join `media:call:42:ana` and send frames into Ana's stream: everyone watching Ana then sees the attacker's video in her place, between her own frames. The program has no way to write a correct rule, because the input it would need is never given.

Repro (`AUDIT/work/L/app1.qwe`; the line is the guide's own):

    route GET /login ? { user: str } = set_session({ user: user })
    media.on("call:*", { publish: (peer) => session().user != null, watch: (peer) => session().user != null })

    quillo run app1.qwe --port 4920 --no-wt
    python3 -I p2_media.py
    # ana joins media:call:42:ana and sends frame ANA-REAL-FRAME; bob (another signed-in user) watches
    # call:42:ana, then joins media:call:42:ana himself and sends BOB-FAKE-FRAME; ana's own viewer tab watches.
    ana join call:42:ana: [(1, True, b'["j","media:call:42:ana",true]')]
    bob (watching ana's private call) got: [b'ANA-REAL-FRAME']
    bob join ana's stream: [(1, True, b'["j","media:call:42:ana",true]')]
    ana's own viewer tab got: [b'ANA-REAL-FRAME', b'BOB-FAKE-FRAME']

| Impact | |
|---|---|
| Functionality | `publish` and `watch` receive the stream's name, so that a rule can bind a name to a user or a room. Rules that take one parameter behave as before. |
| User | In every program built on a `*` stream (the guide's calls), any member can watch any other member's private camera and microphone, and can show their own video under another person's name. |
| Requests | None: one WebSocket and a few messages. |
| Security | Reads other users' live audio and video at will, and impersonates them in calls. The precondition is a `*` stream, which is the documented way to do calls. |

Fix: in `wt::answer`, for a `media:` handler, pass the stream's name. Add it to the peer map (`peer.stream` = `name.strip_prefix("media:")`) or pass it as a second argument (`args_for(f, vec![peer, Value::str(stream)])`). Do the same for a channel's `join` (`peer.channel`). Change the guide's example to check it, for example `publish: (peer, name) => name.ends_with(":" + session().user) && in_room(session().user, room_of(name))`.

### Q-1051. A 146 KB PNG upload makes `image.info` allocate 264 MiB, and a 486 KB one 512 MiB or more, outside every image and memory limit

Found by auditor F (F-1). Checked again by the main session, with no server: `print(image.info(args()[0]))` on a 97 KB PNG whose `iCCP` inflates to 100 MB peaked at 205 MB RSS, against 15 MB for a 101-byte PNG with a 1 KB profile.

Severity: high. Status: CONFIRMED. Entry point: any upload route that calls `image.info` or `image.convert` (the guide's "Images" example, POST /photos). Code: src/imaging.rs:56-64 (`reader`), src/imaging.rs:107 (`info`), src/imaging.rs:335 and :385 (`cost_of` runs before `in_turn`); image-0.25.10 src/codecs/png.rs:50-52; png-0.18.1 src/decoder/stream.rs:1726.

`imaging::limits()` gives the decoder `max_alloc = 512 MiB` for the pixels. The image crate passes that number on to the png crate as its whole budget (`png::Limits { bytes: max_alloc }`), and png inflates an `iCCP` (colour profile) chunk eagerly, while it reads the header, up to that budget (`fdeflate::decompress_to_vec_bounded(buf, self.limits.bytes)`), zero-filling a buffer that doubles as it grows. Zeros deflate about 1000:1, so a 1x1 PNG whose profile inflates to 150 MB is 146 KB on disk, and one that inflates past 512 MiB is about 0.5 MB. This happens in `image.info`, which the guide calls "a cheap check that an upload is an image at all" and which reads "only the header"; it is not under `in_turn` (no limit on how many run at once), the `MOST_PIXELS`/`MOST_BYTES_ONE` estimate does not count it, and `cost_of` decodes the header again before `in_turn` is entered, then `make` a third time. `--max-memory` does not apply (it is no value). An allocation the system refuses aborts the whole process (Rust's allocation-failure handler), and on Linux the OOM killer takes the largest process. On a 1 GB host two uploads at once do it; on a larger one, a burst of a few dozen.

Repro:

    # AUDIT/work/F/img/main.qwe: the guide's "Images" example, copied as written
    #   route POST /photos { ... save_upload(f, `uploads/${id}`)?; let info = image.info(original) ... image.convert(...) }
    $ python3 -I mkpng.py 60000000 uploads/icc60m.png      # 1x1 PNG + iCCP of 60 MB of zeros
    uploads/icc60m.png 58421 bytes on disk, 60000000 bytes of ICC profile
    $ quillo run main.qwe --port 4961 --max-memory 64MB &   # with the RULES' mimalloc caps
    rss before:  15216 KB
    $ curl -F "photo=@uploads/icc60m.png;type=image/png" http://127.0.0.1:4961/photos
    icc60m: 303 0.033833s                                  # accepted as a photo
    max RSS KB during: 149440      rss after: 137872

    $ python3 -I mkpng.py 150000000 uploads/icc150m.png
    uploads/icc150m.png 145893 bytes on disk, 150000000 bytes of ICC profile
    $ curl -F "photo=@uploads/icc150m.png;type=image/png" http://127.0.0.1:4961/photos
    icc150m: 000 0.111771s
    server.log:  memory allocation of 276824064 bytes failed
    $ ps -p $PID  ->  process gone

(Under the RULES' 256 MiB mimalloc cap the 264 MiB allocation was refused and the server process
aborted; without the cap it is taken, and a profile of 512 MiB or more takes the full 512 MiB,
768 MiB while the doubling buffer is copied, before png gives up on it.)

| Impact | |
|---|---|
| Functionality | None for real images: ICC profiles are kilobytes (the largest common ones well under 1 MB). An image with a profile past the new bound is refused, or read without its profile. |
| User | Every visitor loses the site when the process aborts or is OOM-killed; on a host with more memory, other requests slow as it swaps or fails allocations. |
| Requests | One upload of about 0.5 MB per 512 MiB taken; a few at once on a small host, a few dozen on a large one. No limit on concurrent `image.info` calls. |
| Security | A visitor who may upload an image (any app built from the guide's example) takes the whole server down. |

Fix: in `imaging::reader` (so `info`, `cost_of`, `make` and the app icons all get it), for a PNG walk the chunks up to the first `IDAT` and refuse the file when an `iCCP` chunk inflates past a small bound (1-4 MiB, inflated with a bounded output), or give png a budget of its own for metadata (`png::Decoder::new_with_limits` with the pixel budget plus a few MiB, rather than letting `max_alloc` stand for both); and run `image.info` and `cost_of` under `in_turn` (or a smaller permit of their own) so header reads are bounded in number too.

### Q-1052. One server fn call of about 10 KB makes about 1 GB: its arguments are read by `json_value`, which has no value budget and is not held by `--max-memory`

Found by auditor G (G-1). Checked again by the main session: one 1,010-byte gzip call to `/_quillo/rpc/add` (no `--max-memory`) took the server from 22 MB to 94 MB RSS, and was answered `400 missing argument b`: the body is parsed before the arguments are checked.

Severity: high. Status: CONFIRMED. Entry point: `POST /_quillo/rpc/<fn>` (JSON, or multipart with `x-quillo-rpc: 1`) on any program with a server fn; also `/_quillo/wt` channel messages (every program), `POST /_quillo/join` (programs with `QUILLO_SERVICE_SECRET`). Code: src/value.rs:1026 (`json_value`), src/server.rs:2016, src/server.rs:1986, src/wt.rs:370, src/server.rs:1845, src/service.rs:193.

`Value::parse_json` got a budget of values (Q-660) and `--max-memory` counting (Q-977), but
`json_value`, which builds a `serde_json::Value` tree, has neither. With serde_json's
`preserve_order` each value takes 80 bytes, and a list grows by doubling, so `[[0,0,0,...]]` takes
about 100 bytes of memory for each byte of body; then `arg_of` copies the tree into Quillo values.
`/_quillo/rpc/<fn>` runs `json_value` on the whole body (src/server.rs:2016) before the arguments
are checked against the function's types. Under `--max-memory`, the request's start reads a JSON
body through the held reader first (`read_body_held`, src/server.rs:4822), which stops this body at
a small limit (8MB: a 500), but a multipart call's `args` part is a form field there, not JSON, so
the multipart form passes any limit and reaches the same `json_value`. The parse runs on the event
loop, so --instances N bounds how many run at once, and each is that big. The same reader takes
each channel message (1 MiB each) in `from_page` before anything is checked, and
`/_quillo/wt-ticket` and `/_quillo/wt` answer on every program, channels or not; and
`/_quillo/join` parses the body before it checks the signature.

Repro:
    // app.qwe (excerpt)
    server fn add(a: int, b: int) -> int {
        return a + b
    }
    # the body: gzip of [[0,0,...]] with 500,000 / 1,000,000 zeros (1 MB / 2 MB decoded)
    ./srv.sh 4970 base app.qwe --max-memory 64MB
    python3 -I probe.py $PID 4970 POST /_quillo/rpc/add b_rpc_1m.json.gz 'Content-Type: application/json' 'Content-Encoding: gzip'
      in=1158B status='HTTP/1.1 400 Bad Request' time=40ms rss_before=10752KB peak=110304KB  {"error":"missing argument `b` for `add`"}
    python3 -I probe.py $PID 4970 POST /_quillo/rpc/add b_rpc_2m.json.gz ...
      in=2127B status='HTTP/1.1 400 Bad Request' time=93ms rss_before=10960KB peak=202192KB
    # --max-memory 8MB: the JSON form is stopped, the multipart form is not
    ./srv.sh 4971 small app.qwe --max-memory 8MB
    python3 -I probe.py $PID2 4971 POST /_quillo/rpc/add b_rpc_1m.json.gz 'Content-Type: application/json' 'Content-Encoding: gzip'
      in=1158B status='HTTP/1.1 500 Internal Server Error' peak=30160KB
    python3 -I probe.py $PID2 4971 POST /_quillo/rpc/add b_rpc_mp_1m.gz 'Content-Type: multipart/form-data; boundary=BB' 'Content-Encoding: gzip' 'x-quillo-rpc: 1'
      in=1253B status='HTTP/1.1 400 Bad Request' time=18ms rss_before=30160KB peak=90000KB
    python3 -I probe.py $PID2 4971 POST /_quillo/rpc/add b_rpc_mp_3m.gz ... (3 MB decoded)
      in=3191B status='HTTP/1.1 400 Bad Request' time=71ms rss_before=11120KB peak=294304KB
    # channels: a program with none; a ticket from POST /_quillo/wt-ticket, then one 1 MiB text frame
    python3 -I wsprobe.py $PID 4970 1 1
      ticket answer: b'HTTP/1.1 200 OK'   upgrade: b'HTTP/1.1 101 Switching Protocols'
      1 sockets x 1 messages of 1048017 bytes: rss before 14736 KB, peak 89776 KB
    # join, QUILLO_SERVICE_SECRET set, no valid signature
    python3 -I probe.py $PID6 4975 POST /_quillo/join b_join_1m.gz 'Content-Type: application/json' 'Content-Encoding: gzip' 'x-quillo-call: abc123' 'x-quillo-service: t=1,sig=00'
      in=1216B status='HTTP/1.1 401 Unauthorized' time=32ms rss_before=16544KB peak=114368KB

1 MB, 2 MB and 3 MB decoded make 100, 191 and 283 MB more: about 1 GB at the default 10 MB
`--max-body`, from a request of about 10 KB. That one request is enough to get the process
OOM-killed on a host or container with 1 GB. All instances are threads of one process, so the
whole server goes. That last step was not run (PLAUSIBLE): the probe's memory caps did not hold.

| Impact | |
|---|---|
| Functionality | None for real calls: arguments a page sends are small; a call past the budget gets the 400 "the body must be a JSON array of arguments" it gets for bad JSON |
| User | Every visitor loses the server while it is killed and restarted; a deploy's memory cap does not protect it |
| Requests | One ~10 KB request per kill (gzip), or one 10 MB one without gzip; channel messages: 75 MB at a time per 1 MiB message, on the one `quillo-pages` thread |
| Security | Availability: an anonymous visitor takes the whole server down; `--max-memory` (Q-977) does not stop it |

Fix: give `json_value` the same budget and held counting as `Value::parse_json_as` (or read server
fn arguments with `Value::parse_json_held` and turn them into arguments from Quillo values),
counting each serde value at its real size; in the rpc path, take the arguments from the body
already read by `read_body_held` where there is one. In `join_here`, check the signature over the
raw body before parsing it. In `wt::from_page`, use the budgeted reader. Answer `/_quillo/wt-ticket`
only for programs that have channels.

### Q-1053. One 699-byte `check_program` input kills the whole server process (exponential type growth in the checker)

Found by auditor S (S-2). Checked again by the main session, from the command line: N=14 (387 bytes) 0.02 s, 17 MB; N=16 0.06 s, 25 MB; N=18 0.23 s, 65 MB; N=20 (543 bytes) 0.96 s, 179 MB. Each two lines: about 4 times the time and 3 times the memory.

Severity: high. Status: CONFIRMED. Entry point: a route that checks a user's code with `check_program(path, { path: user_code })`, the guide's "plugin editor" use. Also `quillo check` and the LSP on a cloned repository. Code: src/check.rs:341 (`join_part`), src/check.rs:352 (`union_elems`), src/check.rs:357 (`union_shapes`), src/check.rs:253/276 (`elem_id` / `shape_id` tables), src/builtins.rs:3158 (`check_program`, which runs the check with no time or memory budget).

Each line `let xN = [x(N-1), {a: x(N-1)}]` doubles the checker's work and memory. The union of a list's element types recurses into the elements and fields of both sides, with no memo table for pairs it already joined. Every partial union is interned into thread-local tables that are never trimmed during the check. `--max-memory` holds only a request's values, and `--max-compute` does not reach the checker, which runs off the loop. Memory grows until an allocation fails, and Rust then aborts the process. Without the probe's caps, the machine would swap or the OS would kill the process.

Repro:
    # AUDIT/work/S/srv2/main.qwe
    route GET / = "ok"
    route POST /check {
        let found = check_program("plugin.qwe", { "plugin.qwe": request.body })
        return json.stringify(found)
    }

    # bomb26.txt: 699 bytes
    fn f(x: int) { return x }
    let x0 = 1
    let x1 = [x0, {a: x0}]
    ...                      (one such line for each N up to 26)
    let x26 = [x25, {a: x25}]
    let r = f(x26)

    MIMALLOC_RESERVE_OS_MEMORY=256MiB MIMALLOC_ARENA_RESERVE=256MiB MIMALLOC_DISALLOW_OS_ALLOC=1 \
      quillo run main.qwe --port 4941 --max-memory 64MB --max-compute 2s &
    curl -s -o /dev/null -w 'check: %{http_code} %{time_total}s\n' -X POST --data-binary @bomb26.txt \
      -H 'content-type: text/plain' http://127.0.0.1:4941/check

    check: 000 41.066118s
    server process 81063 is gone
    GET / -> 000
    log: memory allocation of 637534174 bytes failed

    Growth, run from the command line (AUDIT/work/S/chk/host.qwe, u4_N.txt):
    N=16: 0.06 s, 27 MB    N=18: 0.25 s, 69 MB    N=20: 1.00 s, 189 MB    (x4 time, about x3 memory per 2 lines)
    Nested `value ? x : (value ? [x] : {k: x})` lines (u5.txt, 60 lines): 30 s of CPU.

| Impact | |
|---|---|
| Functionality | Past a set depth or table size, the checker reports a value's element or fields as unknown (`0`), as it already does for `any`. Real programs see no change. |
| User | Every visitor of the site loses it: the process aborts, including all `--instances`, until it is restarted. |
| Requests | One request of under 1 KB. |
| Security | Any user allowed to have code checked can crash the server at will. A hostile repository does the same to a developer's `quillo check` or editor. |

Fix: memoize `union_elems` and `union_shapes` by `(a, b)`. Stop recursion past a fixed depth of nested element and field types (for example 8) by returning `0`, which means unknown. Give `check_file_opts` a budget when `confined` is set: a limit on `TABLES` entries and on time, which ends the check with one diagnostic ("the code is too complex to check").

### Q-1054. One comment with "data-q-XXXXXXXX" words makes every view of its page compute for seconds to minutes, past `--max-compute`

Found by auditor G (G-2). Checked again by the main session, with `--max-compute 2s`: 10,000 / 20,000 / 40,000 ids took 0.075 / 0.30 / 1.29 s, against 0.001 s for as many `data-x-` words.

Severity: high. Status: CONFIRMED. Entry point: any route that returns an HTML page that shows text a visitor wrote (a comment, a profile, a chat, a preview). Code: src/styles.rs:312-326 (`ids_in`, `if !out.contains(&id)`), called on every HTML answer at src/server.rs:968.

After a route returns HTML, the server looks for the component style ids in the page:
`ids_in` finds every `data-q-` followed by 8 hex digits and keeps the distinct ones in a `Vec`,
checking each new one with `out.contains`. That is quadratic in the number of distinct ids. HTML
escaping leaves `data-q-0000abcd` as it is, so text a visitor wrote can hold as many as it likes.
The work happens after the route has returned, in Rust, so `--max-compute` does not stop it, and it
holds the instance's event loop: other requests to that instance wait. If the text is stored, every
later view pays again, including views by search engines and by the attacker.

Repro:
    // app.qwe (excerpt)
    let comments = []
    route POST /comment {
        comments = comments + [request.body]
        return "saved"
    }
    route GET /comments = <html><body><h1>Comments</h1>{for c in comments => <p>{c}</p>}</body></html>
    route POST /preview = <html><body><h1>Preview</h1><p>{request.body}</p></body></html>

    ./srv.sh 4972 compute app.qwe --max-memory 64MB --max-compute 2s
    python3 -I -c "open('ids_40000.txt','w').write(' '.join('data-q-%08x' % i for i in range(40000)))"   # and data-x- for plain
    curl -H 'Content-Type: text/plain' --data-binary @ids_N.txt localhost:4972/preview
      n=5000   plain 0.000375s   ids 0.018930s     (80 KB)
      n=10000  plain 0.000487s   ids 0.085898s     (160 KB)
      n=20000  plain 0.000780s   ids 0.308291s     (320 KB)
      n=40000  plain 0.001193s   ids 1.327309s     (640 KB)
    curl --data-binary @ids_80000.txt localhost:4972/comment         # 1.28 MB, once
      saved status=200 total=0.001330s
    curl localhost:4972/comments  (twice)
      GET /comments view 1: status=200 size=1280064 total=4.740293s
      GET /comments view 2: status=200 size=1280064 total=4.803295s
    # a GET / sent 0.3 s after a third view starts
      GET / sent 0.3 s later: status=200 total=4.530304s
    log: GET /comments -> 200 (4833.1ms)    (--max-compute 2s did not end it)

Doubling the ids takes about four times as long: a 10 MB comment (`--max-body`'s default; 625,000
ids) would make each view take about 5 minutes. A query string's 64 KiB limit allows about 3,900
ids, 12 ms against 1 ms.

| Impact | |
|---|---|
| Functionality | None: only the program's own style ids (at most a few hundred) need to be found |
| User | The page with the comment, and every page on that instance while it renders, stops answering for everyone |
| Requests | One write of 1.3 MB costs about 5 s of CPU per view (10 MB: about 5 min); a few GETs keep an instance busy |
| Security | Availability: a lasting DoS of a page, then of the server, from one ordinary write; outside `--max-compute` |

Fix: in `styles::ids_in`, check each found id against a `HashSet` (or keep only ids the program's
styles know, as `sheet_for` filters by `by_id` anyway), so the scan is linear.

### Q-1055. A 143-byte `run_script` source freezes the whole instance for about 13 s, again with every request

Found by auditor S (S-1). Checked again by the main session: one `run_script` call on the 143-byte source took 8.5 s of CPU before `the script took too many steps`.

Severity: high. Status: CONFIRMED. Entry point: any route that runs a visitor's script with `run_script` (the guide's documented use). Code: src/script.rs:23 (`SCRIPT_STEPS`), src/interp.rs:5388 (`burn`), src/interp.rs:5399 (`script_builtin`), src/interp.rs:4172 (`script_binop`), src/interp.rs:4159 (`pay_for`).

The guide promises that a script "stops after 20,000 steps (calls and loop rounds)", and that each KB it *makes* costs a step. Work done *inside* one step is not counted. A builtin or operator that walks a large value but gives back a small result costs one step, however long it takes. Examples: `unique` of a list of identical items, `min`, `max`, `sum`, `index_of`, `contains`, and `==` on two lists or maps. A script builds one value of nearly 1 MB, which is allowed, then loops on such a call for the rest of its 20,000 steps. A request that only computes never leaves its instance's event loop (USERGUIDE "Concurrency"), so every other request on that instance waits until the script ends. `--max-compute` is off by default, and the guide's "Scripts from users" section does not mention it.

Repro:
    # AUDIT/work/S/srv/main.qwe
    route GET / = "ok"
    route POST /rule {
        let r = run_script(request.body, { value: 5 })
        return str(r)
    }

    # AUDIT/work/S/cpu/c15.txt (143 bytes)
    let t = join(map(0..300, (i) => "a"), "")
    let xs = split(join(map(0..300, (i) => t), ""), "")
    let n = 0
    while true { n = n + len(unique(xs)) }

    quillo run main.qwe --port 4940 &
    curl -s -X POST --data-binary @../cpu/c15.txt -H 'content-type: text/plain' http://127.0.0.1:4940/rule &
    sleep 0.5; curl -s -o /dev/null -w "GET / during: %{http_code} %{time_total}s\n" http://127.0.0.1:4940/

    GET / during: 200 12.476293s
    script: 200 12.981283s
    log: quillo: an instance has been computing for 5 s without letting go: POST /rule (rid 6fe595d36fc2d162)

    Other scripts, timed in one run_script call each (AUDIT/work/S/host2.qwe, cpu/*.txt):
    `while true { min(xs) }` on xs = 0..65000 (~50 bytes):          1239 ms
    `while true { if xs == ys {...} }`, two 0..65000 lists:          1911 ms
    `a == b` loop on two 40,000-entry maps (349 KB source):        15.9 s (served; GET / waited 14.9 s)
    With `--max-compute 2s` the same script ends at 2.05 s (GET / waited 1.5 s).

| Impact | |
|---|---|
| Functionality | Scripts that compare or scan large values cost more steps. Ordinary rules (a few comparisons on a form's fields) do not change. |
| User | Every visitor on the instance waits about 13 s per hostile request. Without `--instances`, that is the whole site. |
| Requests | One request of 143 bytes buys about 13 s of the event loop. One client sending a request every 13 s keeps the site down. With `--instances 4`, four requests do. |
| Security | Denial of service by any visitor, against the bound the guide promises for `run_script`. |

Fix: in `Interp::script_builtin` and `Interp::script_binop`, charge steps for the size of the lists, maps and texts the builtin or operator reads, as `pay_for` does for what it makes. For example, add one step per 1 KB of `too_big` of each list, map or text argument; the top-level length is enough for lists and texts. Also give `run_script` a wall-clock bound: check `Instant` in `burn` every 256 steps, around 100 ms, or an option. In the guide, recommend `--max-compute` beside `run_script`.

### Q-1056. One visitor on one socket can make the server hold gigabytes: a channel's inbox counts messages (10,000), not bytes (up to 1 MiB each)

Found by auditor L (L-2). Checked again by the main session, by reading: src/wt.rs:81 `INBOX = 10_000`, :83 `MAX_MESSAGE = 1 << 20`, and `Inbox::push` (:530-550) counts messages only.

Severity: high. Status: CONFIRMED. Entry point: `/_quillo/wt` (or WebTransport), `["m", channel, value]` after a join. Code: src/wt.rs:81 (`INBOX = 10_000`), src/wt.rs:419 (each message is pushed as its JSON text), src/wt.rs:536-548 (the only bound is the count), src/wt.rs:83 (`MAX_MESSAGE` = 1 MiB).

A message from a page waits in its channel's inbox until the program's handler has taken it. The inbox is capped at 10,000 messages, and each message may be 1 MiB, so one channel can hold about 10 GiB. Nothing counts these bytes, and they sit outside `--max-memory`. Handlers run one at a time per channel. Any handler that waits (a database write, an `http` call, a `sleep`) drains much more slowly than one socket can fill the queue. A typical chat (`channel.on("chat", (m, p) => db.exec("insert ..."))`) is enough, and so is any channel with no `join` rule, which is the default. The whole process is killed when the machine runs out of memory. The UDP queue has a byte cap (`QUEUE_BYTES`, 64 MiB) for exactly this reason. Channels have none.

Repro (`AUDIT/work/L/app3.qwe`, server capped at `--max-memory 64MB` with the mimalloc caps):

    let lines = 0
    channel.on("chat", (m, p) => {
        sleep(50)                 // stands in for a database write
        lines = lines + 1
    })
    route GET /n = lines

    MIMALLOC_RESERVE_OS_MEMORY=512MiB MIMALLOC_ARENA_RESERVE=512MiB MIMALLOC_DISALLOW_OS_ALLOC=1 \
      quillo run app3.qwe --port 4924 --no-wt --max-memory 64MB
    python3 -I p6_inbox.py 4924 300 <pid>   # anonymous ticket, joins "chat", sends 300 messages of ~1 MiB on ONE socket
    join: [(1, True, b'["j","chat",true]')]
    footprint before 40 MB
    sent 300 messages of ~1 MiB in 0.9s
    handled so far: b'334'  footprint 318 MB
    handled after 5 s: b'426'  footprint 321 MB

(`footprint` is macOS's physical footprint of the server's PID. 300 messages added 280 MB in under a second. The cap is 10,000 messages per channel, and a page may also be in 256 channels.)

| Impact | |
|---|---|
| Functionality | Past a byte budget per inbox (and in all), a new message is dropped and counted, as a full inbox already does by count. Joins and leaves are kept. |
| User | Every visitor loses the site when the process is killed for memory, not only the channel's pages. |
| Requests | One WebSocket. The cost to the attacker is the upload: about 1 GiB per GiB held, and less when the handler waits longer. |
| Security | Denial of service of the whole process by one unauthenticated visitor, where any channel takes messages without a sign-in or the attacker can sign up. |

Fix: in `Inbox::push`, keep a byte count next to `queue`. Past a per-inbox budget (for example 16 MiB) and a process-wide one (for example 256 MiB, or a share of `--max-memory`), drop the new `'m'` event as the count limit does. Charge each peer its own share (as `udp::Queue::admit` does by sender), so that one page cannot take the budget from the others.

### Q-1057. 256 server fn calls of about 10 KB keep about 2.5 GB for good: the live run cache keys each call by its raw argument bytes

Found by auditor G (G-3). Checked again by the main session, by reading: src/server.rs:2057 keys the run by `json_args.clone()`, and `LiveCache::share` (src/livecache.rs:210) sweeps only when there are more than 256 runs.

Severity: high. Status: CONFIRMED. Entry point: `POST /_quillo/rpc/<fn>` with `x-quillo-live: 1`, any server fn that reads nothing of the visitor (a pure function, a database read). Code: src/server.rs:2057 (`key = (scope, name, json_args.clone(), host)`), src/livecache.rs:122 (`RunKey` holds the bytes), src/livecache.rs:210-216 (`share`: old runs are swept only once there are more than 256, at most once a second), src/livecache.rs:173-178 (an old run goes otherwise only when the same key is asked again).

The cache of shared live runs (`LiveCache::runs`) uses the whole argument body as its key: the
decoded body, up to `--max-body`. A run is meant to last 2 s (`SHARED_FOR`), but nothing removes it
unless the same key is asked for again or the map has more than 256 entries. Whitespace padding
inside the JSON array (`[1,2` + spaces + `]`) makes each key different and leaves the arguments
valid, and gzip shrinks it to almost nothing. Up to 256 entries of `--max-body` each stay until the
server restarts, and more stay for 2 s at a time while the calls keep coming.

Repro:
    // app.qwe: server fn add(a: int, b: int) -> int { return a + b }
    # liveleak.py: N calls to add with body gzip([1,2 + (SIZE+i) spaces + ]), then the footprint
    ./srv.sh 4970 base app.qwe --max-memory 64MB
    fresh: Footprint: 6625 KB
    python3 -I liveleak.py $PID 4970 40 2000000 0        # without x-quillo-live
      40 requests, 85137 bytes on the wire, statuses {'HTTP/1.1 200 OK': 40}
      after 40 plain calls + 15s: Footprint: 14 MB
    python3 -I liveleak.py $PID 4970 40 2000000 1        # with x-quillo-live: 1
      40 requests, 85857 bytes on the wire, 0.44s, statuses {'HTTP/1.1 200 OK': 40},
      last body b'{"ok":3,"h":"97346d202973e3bb","topics":[],"seq":1,"server":"9479dcd7"}'
      after 40 live calls + 15s: Footprint: 93 MB
    60 s later:  quillo [68188]: 64-bit    Footprint: 93 MB

In this run, 86 KB on the wire kept 79 MB. At the default `--max-body`, 256 calls of about 10 KB
each keep about 2.5 GB with nothing more sent. RSS falls after a while only because macOS
compresses the idle pages; the footprint shows they are still held.

| Impact | |
|---|---|
| Functionality | None: sharing a run between pages works the same keyed by a hash of the arguments |
| User | The server swaps or is OOM-killed; it stays bloated until restarted |
| Requests | 256 requests, about 2.6 MB in all, for 2.5 GB that stays |
| Security | Availability: a lasting memory hold by an anonymous visitor, needing only a server fn |

Fix: key `runs` and `flights` by a keyed hash of `json_args` (SipHash with the process key
`livecache::hasher`, or SHA-256), not by the bytes; in `LiveCache::share`, sweep expired runs on
every insert (or on a timer), not only past 256; skip sharing calls whose arguments are larger than
a small bound (64 KB).

### Q-1058. Every page state, whatever it holds (an env secret, a whole user row with its password hash), is written into the HTML of each page that has a component, including states no component reads

Found by auditor C (C-2). Checked again by the main session: a page state holding `totp_secret`, shown only as `{me.name}` in the route's HTML, with one component that reads no state: the page carried `totp_secret`; the same page without the component did not; `quillo check` said ok.

Severity: medium. Status: CONFIRMED. Entry point: any page route with a component, in a program with top-level `state`. Code: src/server.rs:893-905 (`page_values` serialized whole), src/client.rs:1949-1956 (the server-data guard exists only for constants).

When a page shows a component, Quillo writes `quillo.g = {...}` with every page state of the request, not only the states the page's components read. Nothing checks where the values came from. If a constant read from `env`, `read_file` or `db` were sent to the browser the same way, the program would refuse to load (`... would be sent to every browser`). Page states get no such check, and `quillo check` says ok. A natural pattern is `state me = load_user()` (to greet the user in the route's HTML) or `state author = db.one(...)` for a post's author. It ships the whole row: password hash, TOTP secret, email, and for `author` another person's. A state set from `env` goes to every visitor.

Repro:

    // pagestate/main.qwe (users row: password_hash, totp_secret, is_admin)
    fn load_me() with db {
        let id = session().user ?? 1
        return db.one("select * from users where id = ?", id)
    }
    state me = load_me()
    state api = env("SECRET_API_KEY") ?? "none"
    component Counter() {
        state n = 0
        <button onclick={() => { n = n + 1 }}>Clicked {n}</button>
    }
    route GET / = <html><body><h1>Hello {me.name}</h1><Counter/></body></html>

    quillo check main.qwe
    # main.qwe: ok (1 route, 1 function, 1 component)
    SECRET_API_KEY=sk_live_PAGESTATE77 quillo run main.qwe --port 4915
    curl -s 127.0.0.1:4915/ | grep -o '<script>[^<]*\.g=[^<]*</script>'
    # <script>(window[Symbol.for("quillo")]||...).g="{\"api\":\"sk_live_PAGESTATE77\",\"me\":{\"id\":1,
    #   \"name\":\"Ana\",\"email\":\"ana@example.com\",\"password_hash\":\"$argon2id$v=19$...HASHHASHHASH\",
    #   \"totp_secret\":\"JBSWY3DPEHPK3PXP\",\"is_admin\":0}}"</script>

`Counter` reads neither state, but both are in the page.

| Impact | |
|---|---|
| Functionality | Only the page states that a component on the page (or in its bundle) reads would travel; a state from env, files, a database or a service that a component reads gets the constants' load error or a check warning |
| User | Password hashes, TOTP seeds, other users' emails and API keys land in page source, browser caches, saved pages and any XSS's reach |
| Requests | None |
| Security | A visitor reads server-only data the developer used only in server HTML, and every visitor reads an env-derived state |

Fix: in the head-building code at src/server.rs:893, serialize only the page states named by the components the page rendered (the client compiler knows which globals each component reads). Run `collect_private`'s test (src/client.rs:1956) over page-state initializers that a component reads: an error at load as for constants, and a `quillo check` warning.

### Q-1059. A translation key from the request is put into the page as HTML by `raw(t(...))` / `t_html` when the key is missing (reflected XSS)

Found by auditor F (F-2).

Severity: medium. Status: CONFIRMED (server); PLAUSIBLE (browser, same code path). Entry point: any route or component that writes `raw(t(`prefix.${value}`))` with a value from the request. Code: src/i18n.rs:420-425 (`translate`: a missing key is the text, then `Value::html` of it); src/client/runtime.js:2970-2986 (`translate`/`B.t_html`: `d.t[k]` else `k`, then `rawHtml`).

`raw(t(key, vars))` is rewritten to `t_html`, which escapes the `{vars}` but treats the translation itself as trusted HTML. When the key is in no locale, the guide says "a key missing there shows as itself", and `translate` returns the key as that trusted text, so the key is written unescaped. The guide itself shows keys built from a value (`` t(`sec.${x}`) ``), so `raw(t(`flash.${request.query.msg}`))` for a flash message with markup is a natural pattern: a link with `?msg=<script>...` runs script on the site. `quillo check` says nothing. No CSP is sent by default, so `<script>` runs.

Repro:

    translations({ en: { flash: { saved: "<b>Saved</b>", deleted: "Deleted {name}" } } })
    route GET /flash { query: { msg: str } } = <div class="flash">{raw(t(`flash.${msg}`))}</div>

    $ quillo check main.qwe
    main.qwe: ok (1 route, 0 functions, 0 components)
    $ curl 'http://127.0.0.1:4960/flash?msg=saved'
    <div class="flash"><b>Saved</b></div>
    $ curl 'http://127.0.0.1:4960/flash?msg=%3Cimg%20src%3Dx%20onerror%3Dalert(document.domain)%3E'
    <div class="flash">flash.<img src=x onerror=alert(document.domain)></div>
    $ curl -D- 'http://127.0.0.1:4960/flash?msg=x%3C/div%3E%3Cscript%3Ealert(1)%3C/script%3E'
    HTTP/1.1 200 OK
    content-type: text/html; charset=utf-8
    ...
    <div class="flash">flash.x</div><script>alert(1)</script></div>

| Impact | |
|---|---|
| Functionality | A missing key in the HTML form shows as escaped text (as `t` shows it), not as markup; no translation that exists changes. |
| User | A visitor who follows a crafted link runs the attacker's script in the site's origin: their session's actions, their data on the page. |
| Requests | One link. |
| Security | Reflected XSS in programs that build a key from request data under `raw(t(...))`; the browser's `t_html` in components has the same fallback. |

Fix: in `i18n::translate` (and `translate` in runtime.js), when `html` is true and the key is found in no locale, return the key escaped (`crate::interp::escape(&key)` / `B.escape(k)`) before interpolation; optionally have `quillo check` warn on `raw(t(...))` whose key is not a literal.

### Q-1060. The Q-581/Q-708 guard misses `query_to_xml`, views and `update pg_settings`: a session setting reaches other visitors on the shared PostgreSQL connection

Found by auditor D (D-3).

Severity: medium. Status: CONFIRMED (private PostgreSQL 17). Entry point: the program's own SQL outside `db.transaction` on PostgreSQL. Code: src/sql.rs:781 (`lasts`), src/db.rs:369 (`session_change`), src/db.rs:398 (`isolated`).

Outside a transaction the shared, pipelined connection is refused anything that changes the session (the Q-581 RLS bypass). The guard reads only the statement's own tokens: `set`, `set_config` by name, temporary tables, advisory locks, `do` blocks, and calls of the database's own functions (run isolated). Three forms change the session and pass:
- `select query_to_xml('select set_config(''app.user_id'', ''alice'', false)', ...)`: a `pg_catalog` function that runs SQL from text, so it is neither read nor isolated (`cursor_to_xml`, `query_to_xml_and_xmlschema` likewise);
- `select v from setter`, where the view `setter` calls `set_config(..., false)`: views, rules and triggers are not looked into;
- `update pg_settings set setting = ... where name = ...`, which PostgreSQL defines as `SET` (session level; works for listed settings such as `search_path` and `statement_timeout`, not for custom `app.*` placeholders).

After one of them, anonymous requests read Alice's rows through the row-level-security policy the guide recommends. Also, `select lastval()` (and `currval`) in one request gives the sequence value of another request's `nextval` on the shared connection.

Repro:
    -- shop: orders(owner, item), RLS: owner = coalesce(nullif(current_setting('app.user_id', true), ''), '-'),
    -- view setter as select set_config('app.user_id', 'alice', false) as v; app is a plain role
    db.open("postgres://app:app@127.0.0.1:4959/shop?sslmode=disable")
    route GET /direct = db.one("select set_config('app.user_id', 'alice', false) as v")
    route GET /xml = db.one("select query_to_xml('select set_config(''app.user_id'', ''alice'', false)', true, false, '')::text as v")
    route GET /view = db.one("select v from setter")
    route GET /tmo = db.exec("update pg_settings set setting = '1' where name = 'statement_timeout'")
    route GET /orders = db.query("select * from orders")

    /direct  -> 500, log: "outside `db.transaction` this connection is shared by every request ..." (guard works)
    /xml     -> 200; then 8 x /orders:  6 x []   2 x [{"id":1,"owner":"alice","item":"alice-secret-ring"}]
    /view    -> {"v":"alice"}; then 8 x /orders: 6 x []   2 x [{"id":1,"owner":"alice","item":"alice-secret-ring"}]
    /tmo     -> {"changes":0}; then /slowq (pg_sleep(0.05)) fails 2 of 8: "canceling statement due to statement timeout [57014]"
    /seq (nextval) -> {"n":1}; /last (lastval) from another request -> {"n":1}
    db.transaction(set_config(..., false)) and db.exec_many(...) are reset: 8 x /who -> {"u":null}

| Impact | |
|---|---|
| Functionality | A few more statements would be run isolated or refused outside a transaction |
| User | One visitor's identity, tenant schema or timeouts apply to other visitors' queries |
| Requests | None extra: it happens on normal traffic once such a statement runs |
| Security | RLS / schema-per-tenant bypass, as Q-581, in programs that set the session this way (uncommon: needs one of these forms in the program's SQL) |

Fix: in `Reader::lasts`, also count `update` of `pg_settings` and calls of `query_to_xml`, `query_to_xml_and_xmlschema`, `cursor_to_xml` and `query_to_xmlschema` as lasting (or isolated, as `unseen` is). For views, rules and triggers no reading can be complete: run `RESET` (`pool::RESET` plus `discard sequences`) after each statement batch on a shared connection whose `pg_settings`/`role` changed, or reset when a lane connection is idle; at least document that a view or trigger that calls `set_config(..., false)` leaks.

### Q-1061. A session from one mounted program is taken as a session by another mounted program's channels

Found by auditor D (D-2).

Severity: medium. Status: CONFIRMED. Entry point: `POST /<mount>/_quillo/wt-ticket`, then `/_quillo/wt?t=...` (WebSocket, or WebTransport). Code: src/wt.rs:1150, src/wt.rs:1174, src/wt.rs:920, src/wt.rs:848.

The guide promises that a mounted program's session "is signed for that mount, so the two programs' sessions never pass for each other". The channel ticket breaks that. `wt::ticket` (wt.rs:1150) seals the session of the mount that made it and the tenant, but not the mount (`session::place`). The ticket is good on any `/_quillo/wt` path. A page's join and messages then go to whichever mount's `channel.on` has that name (`find` searches every mount, wt.rs:920), and run there with the ticket's session as `peer.session` and `session()` (wt.rs:848-856). User 1 of program A (open sign-up) is user 1 of program B (its administrator).

Repro:
    // main.qwe
    mount /a = "a.qwe"
    mount /b = "b.qwe"
    // a.qwe: anyone signs up
    route writes GET /login { set_session({ user: 1 }); "signed in to A as user 1" }
    // b.qwe: user 1 is B's administrator
    channel.on("admin", (msg, peer) => {
        channel.send("admin", peer, { secret: "B admin data", session_seen: peer.session })
    }, { join: (peer) => peer.session.user == 1 })
    route GET /me = session()

    QUILLO_SECRET=... quillo run main.qwe --port 4956 --plain-http
    CK=$(curl -si http://127.0.0.1:4956/a/login | grep -i set-cookie ...)
    curl -H "Cookie: $CK" http://127.0.0.1:4956/b/me          -> {}   (B: no session, as promised)
    curl -X POST -H "Cookie: $CK" -H 'Sec-Fetch-Site: same-origin' http://127.0.0.1:4956/a/_quillo/wt-ticket -> {"ticket":"eyJzIjoie1widXNlclwiOjF9..."}
    node ws.mjs   # WebSocket to /b/_quillo/wt?t=<ticket>, sends ["j","admin"], ["m","admin",{}]
    got: ["j","admin",true]
    got: ["m","admin",{"secret":"B admin data","session_seen":{"user":1}}]

| Impact | |
|---|---|
| Functionality | A channel would hear only pages of its own mount, or see no session from another mount |
| User | B's users' channel data reaches A's users |
| Requests | Two requests and one WebSocket |
| Security | Cross-program authorization bypass on channels in a process that mounts programs (a documented setup: index.qwe mounts every example) |

Fix: in `wt::ticket`, seal `session::place(it)` (the mount and tenant) with the session; in `wt::answer`, give the handler the session only when the ticket's place is the handler's mount (else `{}`), or refuse a join from another mount's ticket.

#### Found again by auditor L (L-5): A channel ticket carries no mount: one program's session is handed to a mounted program's `join` and handlers, so a session a visitor sets freely in one program opens another program's channel

Severity: medium. Status: CONFIRMED. Entry point: `POST /_quillo/wt-ticket` (or `/<mount>/_quillo/wt-ticket`), then `/_quillo/wt`. Code: src/wt.rs:1150-1160 (`ticket` seals `{s, t, e}` with no `base`), src/wt.rs:817-905 (`answer` gives the ticket's session to whichever handler `find` returns), src/wt.rs:920-935 (`find` searches the program, then every mount, by name), src/wt.rs:142-147 (`room_key` holds the tenant, not the mount).

Quillo's sessions are bound to their mount point (src/session.rs:1-17: "a session from one tenant or program is no session in another"). A mounted program indeed sees the root's cookie as `{}`. Channel tickets drop that binding. The root's `/_quillo/wt-ticket` seals the root's session, and the hub matches channel names across the root and all its mounts. A `join` in `/admin`'s program therefore receives a session that `/admin` never made. If the root lets a visitor choose what goes into a session (a display name in a guest book, a cart, a "remember me" value), that visitor can satisfy the mount's `join`. Rooms are also keyed by tenant and name only, so two programs that use the same channel name share one room and the first program's handler.

Repro (`AUDIT/work/L/m/root.qwe` and `m/admin.qwe`):

    // root.qwe
    mount /admin = "admin.qwe"
    route GET /name ? { n: str } = set_session({ user: n })      // a guest picks the name shown beside posts
    // admin.qwe
    route GET /who = session()
    channel.on("ops", (m, p) => channel.broadcast("ops", m), { join: (peer) => peer.session.user == "admin" })
    route GET /bc ? { t: str } = channel.broadcast("ops", { secret: t })

    quillo run m/root.qwe --port 4921 --no-wt
    python3 -I p3_mount.py
    root session set: 204
    mount sees root cookie as session: 200 b'{}'
    ticket payload: {"s":"{\"user\":\"admin\"}","t":null,"e":1795001395}
    join mount's ops: [(1, True, b'["j","ops",true]')]
    got: [(1, True, b'["m","ops",{"secret":"launch-codes"}]')]
    anon join: [(1, True, b'["j","ops",false]')]

| Impact | |
|---|---|
| Functionality | A page's channels are its own program's. A page of the root no longer reaches a mount's channel of the same name, which it could only reach by this accident. |
| User | The members of a mounted program's private channel (staff, admin, a paid area) have their messages read and receive messages from people who never signed in there. |
| Requests | None. |
| Security | Breaks the isolation between mounted programs that sessions promise. It needs a mount with a channel and a session value that some other program lets a visitor choose. |

Fix: in `wt::ticket`, seal `it.base` with the session and tenant. In `wt::answer`, pick the handler of the ticket's program only (the mount at that base, not `find` across all of them). Put the base in `room_key` as well (`base SEP tenant SEP ch`), so that `broadcast`, `send` and `peers` reach their own program's rooms.

### Q-1062. Pointing at a link (65 ms) or the offline worker installing runs a GET's outbound POST, stream or UDP packet as the visitor: the prefetch guard does not count `http.request`, `http.stream` or `udp.send` as writes

Found by auditor N (N-1). Checked again by the main session, by reading: `CHANGES` (src/interp.rs:60-86) lists `http.post`, `put`, `patch`, `delete` but not `http.request`, `http.stream`, `udp.send`, `channel.send` or `channel.broadcast`.

Severity: medium. Status: CONFIRMED (curl and headless Chrome). Entry point: any page that links to
a `GET` route which calls `http.request("POST", ...)` (any method), `http.stream(url, f, { method:
"POST" })` or `udp.send(...)`; Quillo's offline worker's install prefetch of an `offline` route.
Code: src/interp.rs:61-86 (`CHANGES`, the list the guard refuses), src/interp.rs:5033 (the only
check of it); src/fetch.rs:909 (`request`), src/fetch.rs:1075 (`stream`) and src/udp.rs:628
(`send`) never call `no_writes`, unlike `db.rs:2874` for SQL.

The guide promises (USERGUIDE.md, "Prefetching"): "Prefetching changes nothing ... `http.post` and
the like ... stop it, and it is answered 'not prefetched' (503) ... So pointing at a `GET` that
does something never does it." The guard is a fixed list of builtin names. `http.post`, `put`,
`patch` and `delete` are on it; `http.request` (whatever its method), `http.stream` (which takes a
`method` and a `body`), `udp.send`, `channel.send` and `channel.broadcast` are not. Every page
Quillo makes prefetches the same-site links it shows when they are pointed at for 65 ms, with the
visitor's cookies, and Quillo's offline worker prefetches every parameterless `offline` route as it
installs. So a hover performs the write, as the visitor, and the prefetched page is kept for the
click (`cache-control: private, max-age=30`), which then does it again. This is also the guard
Q-965's check says it follows ("as the prefetch guard reads writes").

Repro (AUDIT/work/N/pf, a sink on 4931 logging what it is sent, a UDP listener on 4932):

    route GET /in/:who {
        set_session({ user: who })
        redirect("/")
    }
    route GET /r1 with http {
        http.stream("http://127.0.0.1:4931/stream-post", (d) => print(d), { method: "POST", body: { who: session().user } })
        <html><body>r1 done</body></html>
    }
    route GET /r2 with http {
        http.request("POST", "http://127.0.0.1:4931/request-post", { who: session().user })
        <html><body>r2 done</body></html>
    }
    route GET /r3 with http {
        http.post("http://127.0.0.1:4931/post", { who: session().user })
        <html><body>r3 done</body></html>
    }
    route GET /r4 with net {
        udp.send("127.0.0.1", 4932, "vote " + str(session().user))
        <html><body>r4 done</body></html>
    }

    $ curl -s -c jar http://127.0.0.1:4930/in/alice
    $ for p in /r1 /r2 /r3 /r4; do curl -s -b jar -H 'x-quillo-prefetch: 1' -H 'Sec-Purpose: prefetch' -D - http://127.0.0.1:4930$p; done
    /r1  HTTP/1.1 200 OK   r1 done
    /r2  HTTP/1.1 200 OK   r2 done
    /r3  HTTP/1.1 503 Service Unavailable          <- http.post is refused, as documented
    /r4  HTTP/1.1 200 OK   r4 done
    sink:  POST /stream-post {"who":"alice"}
           POST /request-post {"who":"alice"}
    udp:   vote alice

In Chrome (pf/hover.mjs): a page `<a id="go" href="/notify">Tell the team</a>`, where `GET /notify`
does `http.request("POST", "http://127.0.0.1:4931/team-chat", { text: str(session().user) + " was
here" })`. The mouse is moved over the link and never pressed:

    on: http://127.0.0.1:4930/ | Tell the team
    still on: http://127.0.0.1:4930/ (the link was only pointed at, never clicked)
    sink got: POST /team-chat {"text":"alice was here"}
    server log: GET /notify -> 200 (7.9ms) prefetch

| Impact | |
|---|---|
| Functionality | a GET that calls `http.request` with a method other than GET/HEAD/OPTIONS, `http.stream` with a `method`/`body`, `udp.send`, `channel.send` or `channel.broadcast` is answered "not prefetched" (503) to a prefetch and to the worker's install, and runs when the link is followed, as `http.post` already does |
| User | an outbound action they never chose (a chat message, a paid API call, a vote, an LLM request billed to the site) is sent when they move the mouse over a link, and again on the click |
| Requests | none from an attacker: any page of the site that shows such a link; a page with many such links fires them as they are hovered |
| Security | the prefetch guard's promise is broken for outbound writes: a hover (or the offline worker's install) does them with the visitor's identity |

Fix: add `http.request` (when its method, read at run time, is not GET/HEAD/OPTIONS), `http.stream`
(when its `method` is not GET or it has a `body`), `udp.send`, `udp.reply`, `channel.send` and
`channel.broadcast` to the guard: call `it.no_writes(name, line)?` at the top of
`fetch::request` and `fetch::stream` for a writing method, and in `udp::send_from` and the channel
senders, under `it.writes_noted()`; and have `quillo check`'s `GetWalk::call` count the same (see
Q-1084).

### Q-1063. The bundle's guard against server-only constants misses plugin functions, `args()`, `crypto.*`, `read_line`/`stdin` and site calls: their values go into the public JavaScript bundle

Found by auditor C (C-3).

Severity: medium. Status: CONFIRMED (for `args()` and `crypto.token()`; the plugin path is traced). Entry point: `GET /_quillo/b/<any>.js` (the whole bundle, served to anyone). Code: src/client.rs:1949 (`PRIVATE_SOURCES = ["env", "read_file", "file", "db", "http"]`), src/client.rs:1956-2040 (`collect_private`), src/client.rs:2687.

A component that reads a top-level constant gets the constant's value written into the bundle. The guard refuses this only when the value comes from `env`, `read_file`, `file`, `db`, `http`, a service, or a program function that uses one of those. A value from a native plugin function is not caught. The catalog ships `dotenv_read_file`, `vault_kv_get` and `aws_secrets` functions, and `let CFG = dotenv_read_file(".env.production")?` with a header that shows `CFG.PUBLIC_SITE_NAME` would put the whole file in the bundle. Also not caught: `args()` (command-line secrets), `crypto.token()` and other generated keys, `read_line`/`stdin`, `run_script`, and `site` calls. The bundle for an unknown hash is the whole program's, so the value is public even if only an admin page's component reads it.

Repro:

    // bundle/main.qwe
    let SIGNING = crypto.token()
    let ARGS = args()
    component Pay() {
        state n = 0
        <button onclick={() => { n = n + 1; print(SIGNING); print(ARGS) }}>Pay {n}</button>
    }
    route GET / = <html><body><Pay/></body></html>

    quillo run main.qwe --port 4913 -- --secret-arg=hunter2
    curl -s 127.0.0.1:4913/_quillo/b/0000.js > full.js
    grep -o hunter2 full.js
    # hunter2
    grep -o -E '"[A-Za-z0-9_-]{43}"' full.js
    # "NJNx8pnEcynKBCKQ2UUmgMo39xGHnqu5uBNTSxj0E6M"      <- the server's crypto.token()

    (For comparison, `let K1 = load_key()` where `load_key` calls `env`, and a constant from a
    `use`d module that reads `env`, both stop the load: "`K1` is read from the server's
    environment ... would be sent to every browser".)

The plugin case: `plugin/main.qwe` (`let CFG = dotenv_read_file(".env.production")?` shown by a component) could not be run. The catalog's prebuilt dylib was made by an older Quillo, and loading it needs cargo. `collect_private` matches only identifiers in `PRIVATE_SOURCES`, services, and the program's own `fn`s that use them, so a `use "x.qwer"` function's result is not marked.

| Impact | |
|---|---|
| Functionality | A component that reads such a constant would get the load error that `env`-derived constants get (pass it as a prop or through a server fn) |
| User | A secret the developer believed stayed on the server is in a public `.js` file |
| Requests | One GET of `/_quillo/b/x.js` |
| Security | Anyone reads API keys from `.env` files or vaults read through plugins, generated signing keys, and command-line secrets |

Fix: in `collect_private`, count as private any call of a native or plugin function (`native:` builtins, `use`d `.qwer`/`.rs` functions), `args`, `read_line`, `stdin`, `run_script`, `crypto.*`, and site calls (`Stmt` for `site`). The rule should be "private unless pure", not a list of sources.

### Q-1064. Under `--split`, every visitor reaches a service's routes as 127.0.0.1, so a `request.ip` check that refuses them in one process lets them through

Found by auditor C (C-1).

Severity: medium. Status: CONFIRMED. Entry point: any route under a split service's prefix (`/shop/...`), passed on by the app. Code: src/service.rs:2338, src/httpd.rs:2312.

`pass_on` sends the visitor's address to the service as a plain `x-forwarded-for` header. A service that `--split` starts gets no `--trust-proxy`, so `client_of` ignores that header. The service then sees the app's loopback connection, and every visitor's `request.ip` is `127.0.0.1`. The same service code in one process sees the real address. A route that allows only this machine (`request.ip != "127.0.0.1"` gives 403) therefore lets in everyone once the app runs with `--split`. Every per-address limit at the service also puts all visitors into one bucket: live pages waiting (256 per client, src/server.rs:2523), large files being sent (8 per client), and `--connections-per-client`. One visitor can fill the bucket for everyone. The app already passes on its scheme decision signed (`SCHEME_WORD`), but not the client's address.

Repro:

    // split/app.qwe
    service /shop = "shop.qwe"
    route GET /whoami = { ip: request.ip, xff: request.headers["x-forwarded-for"] }
    // split/shop.qwe
    route GET /whoami = { ip: request.ip, host: request.host, headers: request.headers }
    route GET /admin {
        if request.ip != "127.0.0.1" { reply 403 "local only" }
        return "admin panel"
    }

    quillo run app.qwe --port 4911 --split --trust-proxy 127.0.0.1
    curl 127.0.0.1:4911/whoami -H 'X-Forwarded-For: 203.0.113.9'
    # {"ip":"203.0.113.9","xff":"203.0.113.9"}          <- the app knows the visitor
    curl 127.0.0.1:4911/shop/whoami -H 'X-Forwarded-For: 203.0.113.9'   (ip, x-forwarded-for)
    # 127.0.0.1 203.0.113.9                             <- the service sees loopback
    curl 127.0.0.1:4911/shop/admin -H 'X-Forwarded-For: 203.0.113.9'
    # admin panel

    quillo run app.qwe --port 4911 --trust-proxy 127.0.0.1      (same files, one process)
    curl -i 127.0.0.1:4911/shop/admin -H 'X-Forwarded-For: 203.0.113.9'
    # HTTP/1.1 403 Forbidden ... local only

| Impact | |
|---|---|
| Functionality | `request.ip` in a split service would be the visitor's address, as in one process; nothing changes for programs that run in one process |
| User | A developer's IP allowlist or per-IP limit works in development and fails open in production under `--split`; one heavy visitor uses up a service's per-client places for everyone |
| Requests | None extra; at most 256 live long polls from one client fill the service's shared bucket |
| Security | Anyone on the internet passes "this machine only" and per-address checks in a split service's routes |

Fix: in `service::pass_on`, send the app's `request.remote` as a signed word, the way `scheme_word` does it. The service then takes it in `httpd::client_of` (or where `remote` is set) from a local peer when `takes_app_word()` is true, and never takes the plain `x-forwarded-for`.

### Q-1065. Under `--split`, while a service is down (a crash or a deploy), anyone can join that service's channel and keep hearing it after the service returns

Found by auditor L (L-6).

Severity: medium. Status: CONFIRMED. Entry point: `/_quillo/wt`, `["j", <service's channel>]` in a `--split` app. Code: src/wt.rs:713-741 (`remote_for`: a miss asks the services again and replaces the list, dropping the channels of a service that did not answer, and for 2 s nobody asks again), src/wt.rs:821-840 (`answer`: no handler and no service, so `"j" => verdicts.push(!name.starts_with("media:"))`, a yes).

In the app, a channel nobody names may be joined by design: it hears the program's broadcasts. A split service's channel is recognised only through the app's cached list of names. Any join of an unknown name re-asks the services, if the last ask is more than 2 s old, and replaces the list. A service that does not answer at that moment (it is restarting: "starting it again in 1 s") loses all its names. For the next 2 s, a join to the service's channel takes the "nobody names it" path and is let in without the service's `join`. The page stays in the room once the service is back, and receives everything the service broadcasts there. An attacker only has to send a junk join followed by the real one every two seconds, and wait for a restart.

Repro (`AUDIT/work/L/split/`):

    // app.qwe
    service chat = "chat.qwe"
    route GET /announce ? { t: str } with http = chat.announce(t)
    // chat.qwe
    channel.on("staff", (m, p) => channel.broadcast("staff", m), { join: (peer) => peer.session.user == "boss" })
    server fn announce(t: str) = channel.broadcast("staff", { secret: t })

    quillo run split/app.qwe --port 4929 --no-wt --split
    python3 -I p9_split.py <service pid>      # service up: anonymous join refused
    anon join, service up: [(1, True, b'["j","staff",false]')]
    python3 -I p9b_split.py <service pid>     # SIGKILL the service (standing in for a crash), join "no-such-channel", then "staff"
    service down (a crash or a deploy)
    miss answered: [(1, True, b'["j","no-such-channel",true]')]
    anon joins staff while the service is down: [(1, True, b'["j","staff",true]')]
    announce once the service is back: 200 b'1'
    anon page got: [(1, True, b'["m","staff",{"secret":"payroll-file"}]')]
    # log: quillo: service chat stopped (signal: 9 (SIGKILL)); starting it again in 1 s

| Impact | |
|---|---|
| Functionality | Under `--split`, a join to a name no reachable program claims is refused while any service is unreachable, and the last known names are kept until a service answers again. |
| User | A service's private channel (staff, a paid room) is heard by anonymous pages after any restart of that service. |
| Requests | One join every 2 s until the service restarts. |
| Security | Bypasses the service's `join`, so the attacker reads its broadcasts. It needs `--split` and an outage window, which every deploy or crash provides. |

Fix: in `wt::remote_for`, merge a failed ask into the cached list rather than replacing it (keep a service's names until it answers with new ones). In `wt::answer`, when `SPLIT` is non-empty and some service could not be asked, answer `false` to a `'j'` for a name no handler claims.

### Q-1066. `media.rtp` listens on every interface by default and takes any sender's packets, so anyone who reaches the UDP port puts their video into the stream every viewer sees

Found by auditor F (F-3).

Severity: medium. Status: CONFIRMED (bind and acceptance from an unrelated sender on loopback; a remote sender takes the same path). Entry point: UDP port given to `media.rtp`. Code: src/media.rs:704 (`let mut host = "0.0.0.0"`), src/media.rs:608 (`UdpSocket::bind((host, port))`), src/media.rs:632 (`Ok((n, _))`: the source address is dropped, no SSRC or payload-type check).

`quillo run` listens on 127.0.0.1 unless `--host` says otherwise, but `media.rtp(name, { video, audio })` binds `0.0.0.0` unless `host` is given, and the guide's example gives none (its ffmpeg line sends to 127.0.0.1). RTP has no authentication and the receiver keeps no source: any host on the internet that reaches the port can send H.264 frames that go out to every page watching (shown to logged-in users through `media.on`'s `watch`), or keep the stream broken (packets far ahead make the reorder buffer start again; a broken frame makes viewers wait for a keyframe). The guide says "keep the ports to the network the camera is on", while the default does the opposite of what the HTTP side does.

Repro:

    media.rtp("cam", { video: 4969 })
    media.on("cam", { watch: (peer) => peer.session.user != null })
    route GET /streams = media.streams()

    $ quillo run main.qwe --port 4960 &
    $ lsof -nP -iUDP:4969 -a -p $PID
    quillo  76031 restiguay    3u  IPv4 0x53f502464e1a31a9      0t0  UDP *:4969
    $ curl http://127.0.0.1:4960/streams
    [{"name":"cam","viewers":0,"video":{"frames":0,...}}]
    $ python3 -I rtp.py 4969            # SPS, PPS, IDR + 4 frames from an unrelated socket, SSRC 0xdeadbeef
    sent 15 packets from an unrelated socket
    $ curl http://127.0.0.1:4960/streams
    [{"name":"cam","viewers":0,"video":{"frames":5,...}}]

| Impact | |
|---|---|
| Functionality | A program that takes RTP from another machine names `host: "0.0.0.0"` (or the camera network's address), as `--host` is named for HTTP today. |
| User | Viewers are shown whatever an outsider sends (offensive or misleading video), or a stream that keeps freezing. |
| Requests | A few UDP packets a second from anywhere that reaches the port. |
| Security | Content injection into a live stream and its disruption, with no account, for any program that copies the guide's example onto a host without a firewall. |

Fix: in `media::rtp`, default `host` to `127.0.0.1` (or to the server's `--host`), and in `receive` keep the first sender's address and SSRC per track, dropping packets from others until the track is idle (or take an `allow` list of sender addresses).

### Q-1067. One slow reader of a stream sent inside a SQLite transaction stops the whole instance, and a few connections keep it stopped

Found by auditor D (D-1). Checked again by the main session: with one client that sent `GET /export.txt` and read nothing for 8 s, `GET /ping` took 7.0 s (0.0003 s before).

Severity: medium (the auditor said high; lowered by the main session, since only a program that streams inside a SQLite `db.transaction` is open to it). Status: CONFIRMED. Entry point: any `stream(...)`/`events(...)` route whose `send` runs inside `db.transaction` (the shape of Q-647's report). Code: src/httpd.rs:2080, src/httpd.rs:808, src/httpd.rs:53, src/db.rs:3159, src/interp.rs:2697.

This is the other side of the Q-647 fix. A SQLite transaction sets `hold_in_place(true)` (db.rs:3159), so `wait_elsewhere` refuses to pause the request (httpd.rs:811), and `ConnSink::write` → `writable()` (httpd.rs:2080-2090) falls back to `poll()` in place for up to `SLOW` = 30 s (httpd.rs:53) while holding the interpreter lock. Nothing else in the instance runs meanwhile: not even a route that touches no database. The guide (line 7233) says `sleep`, `http` and queries to another database hold up other requests inside a SQLite transaction, but says nothing about a stream's `send`, and `quillo check` does not warn. Each idle connection holds about 35 s; the requests queue on the lock, so an attacker opening N connections that never read stops the instance for about N × 35 s, and can repeat. `--max-compute 2s` (off by default) shortens each hold to about 5 s.

Repro:
    // work/D/q647/main.qwe (2,000 rows of 10 KB)
    route GET /export.txt = stream(send => db.transaction(() => for row in db.query("select body from items") { send(row.body) }))
    route GET /ping = "pong"

    quillo run main.qwe --port 4954
    # slow.py: connect, send "GET /export.txt", read nothing, sleep N s
    python3 -I slow.py 12 &  ;  time curl -s http://127.0.0.1:4954/ping
    pong   ... 11.032 total
    for i in 1 2 3; do python3 -I slow.py 110 & done ; curl -s -w '%{time_total}' http://127.0.0.1:4954/ping
    ping: 200 after 104.422957s
    log: GET /export.txt -> 200 (35176.8ms) x3, then GET /ping -> 200 (0.0ms)
    with --max-compute 2s: ping: 200 after 4.163290s

| Impact | |
|---|---|
| Functionality | A stream inside a SQLite transaction could no longer wait for a slow client while holding everyone; it would buffer, or fail fast |
| User | Every visitor of the instance waits or times out while one client does not read |
| Requests | One idle connection per ~35 s, no bandwidth: a handful of sockets keeps a site down |
| Security | Denial of service of the whole instance (the whole process at the default `--instances 1`) |

Fix: in `ConnSink::write`, when `held()` is true, do not wait for the socket: keep the stream's output in memory up to a cap (and fail `send` past it, ending the transaction), or wait with a short limit (say 1 s) and fail the `send` as a client gone. Add a `quillo check` warning for `send` inside `db.transaction`, and a line in the guide next to the one on `sleep` and `http`.

### Q-1068. `stream` and `events` send a builtin error's text to the visitor (database errors, internal URLs with keys), where routes, server fns and channels hide it

Found by auditor L (L-7).

Severity: medium. Status: CONFIRMED. Entry point: any route answering `events((send) => ...)` or `stream((send) => ...)`. Code: src/stream.rs:204-258 (`encode`: `json_into_bounded` on the value as given), src/stream.rs:282 (`send` calls it, with no `hidden`/`for_visitor`). Compare src/wt.rs `message` (Q-773) and src/server.rs's route and server-fn answers (Q-161), which hide it.

The error rule ("only errors the program wrote reach a visitor", Q-161 for routes, Q-773 for channels) is not applied to `send`. A value that contains a builtin's error (`db.*`, `http.*`, file reads) and goes out through a stream or an event stream carries the error's full text. That text includes table and column names, file paths, and the full URL of an internal call, API keys in its query included. Progress streams that send each step's result (`send({ step, result })`) are where this happens.

Repro (`AUDIT/work/L/app5.qwe`):

    server fn fetch_report() = http.get("http://127.0.0.1:4927/internal/report?api_key=sk_live_SECRET123")
    route GET /plain = { result: fetch_report() }
    route GET /ev = events((send) => { send({ step: 1, result: fetch_report() }) })
    db.open(":memory:")
    route GET /dbplain = { rows: db.query("select secret_col from users_private where id = 1") }
    route GET /dbev = events((send) => send({ rows: db.query("select secret_col from users_private where id = 1") }))

    quillo run app5.qwe --port 4928 --no-wt
    curl -s http://127.0.0.1:4928/plain
    {"result":{"error":"internal error"}}
    curl -s http://127.0.0.1:4928/ev
    data: {"step":1,"result":{"error":"http: http://127.0.0.1:4927/internal/report?api_key=sk_live_SECRET123: Connection Failed: Connect error: Connection refused (os error 61)"}}
    curl -s http://127.0.0.1:4928/st        (the same with stream)
    {"step":1,"result":{"error":"http: http://127.0.0.1:4927/internal/report?api_key=sk_live_SECRET123: ..."}}
    curl -s http://127.0.0.1:4928/dbplain
    {"rows":{"error":"internal error"}}
    curl -s http://127.0.0.1:4928/dbev
    data: {"rows":{"error":"db: no such table: users_private"}}
    # a channel.send of the same value says {"error":"internal error"} (checked)

| Impact | |
|---|---|
| Functionality | A builtin's error inside a streamed value is written as `internal error` and logged, as everywhere else. Errors the program wrote are unchanged. |
| User | The developer's secrets and schema reach visitors through any stream that forwards a result. |
| Requests | One request. |
| Security | Leaks internal URLs, credentials in query strings, schema, and paths. It needs a stream that sends a value holding a builtin error. |

Fix: in `stream::send`, before `encode`, replace the value with `crate::server::hidden(v)` where `crate::value::hiding()` is set, and log the original, as `wt::message` does.

### Q-1069. The markdown door, on every server, makes 160-220 MB from a 1.2 KB request, past `--max-memory`

Found by auditor G (G-4).

Severity: medium. Status: CONFIRMED. Entry point: `POST /_quillo/markdown` (every program; JSON; no Origin needed). Code: src/server.rs:1898-1914 (1 MB check on the decoded body, line 1912), src/builtins.rs:3807 (`Parser::new_ext`, pulldown-cmark 0.13.6).

Q-706 limited this door to 1 MB of JSON and bounded the highlighting by `--max-compute`, and
Q-977 holds what is written out. But pulldown-cmark first builds its own tree of the whole text,
and for nested openers that tree costs about 160 bytes per input byte. One megabyte of `[`
(unclosed link openers) or `>` (nested block quotes) makes 160-200 MB. That tree is not held by
`--max-memory`. The size check is on the decoded body, so gzip makes the request about 1 KB.

Repro:
    python3 -I -c "import json; open('md/brackets1m.json','w').write(json.dumps(['['*((1<<20)-40)]))"   # and '>' for quote1m; gzip -9
    ./srv.sh 4971 small app.qwe --max-memory 8MB
    python3 -I probe.py $PID2 4971 POST /_quillo/markdown md/brackets1m.json.gz 'Content-Type: application/json' 'Content-Encoding: gzip'
      out=1048790B in=1205B status='HTTP/1.1 200 OK' time=95ms rss_before=11024KB peak=173920KB
    python3 -I probe.py $PID2 4971 POST /_quillo/markdown md/quote1m.json ... (not gzipped)
      in=1048668B status='HTTP/1.1 500 ...' time=55ms rss_before=10656KB peak=172080KB  {"error":"this request's value would be bigger than --max-memory allows (8388608 bytes)"}
    # --max-memory 64MB: quote1m answers 200 in 175 ms server-side with a 30 MB answer; RSS peak 199-217 MB

| Impact | |
|---|---|
| Functionality | None for previews: no real text nests thousands of levels deep |
| User | Memory spikes on every Quillo server; with --instances N, N at once |
| Requests | One ~1.2 KB request makes about 170 MB for about 0.1 s; repeated, it keeps a small host short of memory |
| Security | Availability; a bypass of Q-706/Q-977's limits on a door every program has |

Fix: in the `markdown` branch of `handle_internal`, refuse text whose openers nest past a bound
(a run of more than, say, 100 `>`, `[` or list markers at one place), or lower `MARKDOWN_BODY` to
64 KB for this door. Answer the door only for programs whose pages use `markdown` (runtime.js
`B.markdown`).

### Q-1070. A 3 KB request to any path makes 136 MB when `--max-memory` is set: JSON's value budget has a floor of 1,000,000, and an empty map or list is counted as one slot

Found by auditor G (G-7).

Severity: medium. Status: CONFIRMED. Entry point: any POST with a JSON body: every path (404s, routes that never read the body) under `--max-memory`; routes that read `request` otherwise. Code: src/value.rs:962 (`budget = (text.len() / 8).max(JSON_VALUES)`), src/value.rs:1156 (`push_held` counts the list's slot, not the empty map it holds), src/server.rs:4822 (`read_body_held` for every body of 1,260 bytes or more).

Q-660's budget allows at least 1,000,000 values whatever the body's size, so 3 MB of `{},` reaches
it. Each `{}` is a map in its own `Arc`, about 136 bytes; `--max-memory` counts only the list's
16-byte slots. With `--max-memory` set, `read_body_held` reads every body of 1,260 bytes or more at
the request's start, before routing, so the cost lands on paths that never read it. Gzip makes the
request 3 KB.

Repro:
    python3 -I -c "import gzip; open('b_maps_1m.json.gz','wb').write(gzip.compress(b'['+b','.join([b'{}']*1000000)+b']',9))"   # 2,948 bytes
    ./srv.sh 4970 base app.qwe --max-memory 64MB
    python3 -I probe.py $PID 4970 POST /ignore b_maps_1m.json.gz 'Content-Type: application/json' 'Content-Encoding: gzip'
      in=3087B status='HTTP/1.1 200 OK' time=72ms rss_before=11584KB peak=148128KB body=b'ignored'
    python3 -I probe.py $PID 4970 POST /no-such-path b_maps_1m.json.gz ...
      in=3093B status='HTTP/1.1 404 Not Found' time=62ms rss_before=11936KB peak=148032KB
    python3 -I probe.py $PID 4970 POST /json b_lists_1m.json.gz ...      ([[],[],...])
      in=3083B status='HTTP/1.1 200 OK' time=47ms rss_before=11760KB peak=110208KB body=b'1000000'

The cost is bounded at about 180 MB per request (1.31M values for a 10 MB body). It is transient.

| Impact | |
|---|---|
| Functionality | A JSON body of a million empty maps would be refused (400 or `request.json` null), as any body past the budget is |
| User | Memory spikes; with --instances N, N at once |
| Requests | One 3 KB request makes 136 MB for about 70 ms |
| Security | Availability; `--max-memory` makes it worse by reading bodies no route asked for |

Fix: count the budget in bytes, not values: charge each list or map its own size (an empty one
about 64 bytes) and drop the 1,000,000 floor for bodies (keep it for `json.parse` in programs);
in `Worker::answer`, read the body held only when a route or builtin asks for it, as without the
limit.

### Q-1071. A split service's answer is read whole into the app's memory (up to 256 MB a request, not held to `--max-memory`): a few downloads of one large file take the app's memory

Found by auditor C (C-4).

Severity: medium. Status: CONFIRMED. Entry point: any route under a split service's prefix that answers a large body (`file(...)`, a public file, a long `stream`). Code: src/service.rs:2354 (`take(256 << 20).read_to_end`), src/service.rs:2316.

`pass_on` reads the service's whole answer into a `Vec` before it sends anything. The read runs in `unlocked`, outside the `Held` accounting, so `--max-memory` does not apply. In one process, the same `file()` goes out from disk on a thread of its own (`send_file`). A single 60 MB download raised the app's RSS by about 145 MB. Three at once raised it to 633 MB, and the memory stayed taken after they ended. `stream`/`events` answers from a service are held until they end or the 60 s timeout, so they never stream through the app. The same code also duplicates repeated response headers: `headers_names()` lists `set-cookie` once per line, and each time `resp.all()` adds every line, so N cookies become N² headers (seen in the `/shop/setc` test: each `Set-Cookie` twice).

Repro:

    // split/shop.qwe:   route GET /big = file("big.bin")      (big.bin: 60 MB)
    // split/app2.qwe:   service /shop = "shop.qwe"
    quillo run app2.qwe --port 4911 --split
    ps -o rss= -p $APP          # 16608 KB
    curl -s -o /dev/null 127.0.0.1:4911/shop/big     # 200, 62914560 bytes
    ps -o rss= -p $APP          # 161392 KB
    (three at once)             # 633184 KB while running, 630064 KB after

| Impact | |
|---|---|
| Functionality | Answers would be streamed through (status and headers first, then the body as it comes), so services' streams and large files work under `--split` as they do in one process |
| User | The whole app (every route, not only the service's) runs out of memory or is killed; streamed pages of a split service do not stream |
| Requests | About 20 concurrent requests for a 256 MB file reach several GB; each held request also keeps an app wait-pool thread for its whole transfer |
| Security | An anonymous visitor can take the app down if any split service serves a large public body |

Fix: in `service::pass_on`, return the head and then stream `resp.into_reader()` into the connection. Use the stream mechanism the app already has for `stream` answers, or at least cap the body at `--max-body`/`--max-memory` and answer 502 beyond it. Build the headers by iterating once over `headers_names()` deduplicated (or keep only `resp.all` per unique name).

### Q-1072. A 200 KB gzip answer makes each `http.*` call hold about 200 MB; `public_only` does not help, and no budget limits calls at once

Found by auditor F (F-4).

Severity: medium. Status: CONFIRMED. Entry point: any route that fetches a URL a visitor chooses (link previews, avatars from a URL, webhook targets: the case the guide answers with `public_only: true`). Code: src/fetch.rs:520 (`reader.take(most + 1).read_to_end(&mut bytes)`), src/fetch.rs:595-614 (`decoded`), src/fetch.rs:48 (`MAX_BODY` 64 MiB); the same at src/fetch.rs:1131 for `http.stream`'s non-2xx body.

The answer is decoded as it is read and cut at 64 MiB (or `--max-memory`), which is right, but `read_to_end` grows its buffer by doubling, so reaching 64 MiB + 1 makes a 128 MiB buffer and copies the old one: about 200-230 MB of RSS per call, for 200 KB sent by the attacker's own public server. Calls run off the interpreter lock on up to `--workers` (64) threads, each with its own buffer, so 64 visitor requests at once ask for about 13 GB. Without `--max-memory`, which is off by default, nothing else stops it.

Repro:

    route GET /get { query: { u: str } } {
        let r = http.get(u, {}, { public_only: false })
        if is_err(r) { return `ERR ${str(r)}` }
        `${r.status} len=${len(r.body)}`
    }
    # listener.py on 127.0.0.1:4964: /gzbomb = Content-Encoding: gzip, 200 MiB of zeros (203 KB sent)

    $ curl 'http://127.0.0.1:4965/get?u=http%3A%2F%2F127.0.0.1%3A4964%2Fgzbomb'
    ERR err("internal error")  [0.036665s]
    server.log: http: http://127.0.0.1:4964/gzbomb: the answer is longer than 64 MiB, the most that is read whole
    gzbomb max RSS KB 230464        (from 15136 before)
    dfbomb (deflate) max RSS KB 226480
    two calls at once:  max RSS KB 395056

| Impact | |
|---|---|
| Functionality | None for answers under the limit; a long answer is refused as now, without the doubled buffer. |
| User | The process is OOM-killed, or the host swaps: every visitor loses the site. |
| Requests | Some tens of requests at once, each answered by a 200 KB file on any public host. |
| Security | A visitor takes the server down through any feature that fetches a URL they give, `public_only` or not. |

Fix: in `fetch::send` (and the non-2xx read in `stream`), read into a buffer that never grows past the limit (read in 64 KiB pieces with `try_reserve_exact` up to `most + 1`, or a `Vec::with_capacity` from `Content-Length` when it is given and small), and hold the bytes of answers being read to a process-wide budget (for example 256 MiB, as `in_turn` holds images), past which a call waits or gets an error value.

### Q-1073. A visitor can hold about 1 MiB of server memory per channel socket indefinitely by never finishing a message: 256 sockets per address, 4,096 in all, outside `--max-memory`

Found by auditor L (L-3).

Severity: medium. Status: CONFIRMED. Entry point: `/_quillo/wt` (WebSocket). Code: src/wt.rs:1549 (`Reader` with `MAX_MESSAGE + 64` and no deadline), src/ws.rs:936-1000 (`read_message`: `vec![0u8; len]` per frame, and parts gathered in `partial` until `fin`).

The channel socket's reader waits as long as it takes for the rest of a message. It has no read deadline, unlike the live socket's `heard`, which gives a started frame 10 s. It gathers a fragmented message whole, up to 1 MiB + 64. A client that sends 1 MiB minus a few bytes of a text message in parts, and never the last part, keeps that memory allocated for as long as the TCP connection stays up. The server's pings do not need an answer. One anonymous ticket can be reused for every socket for 120 s. Each address (or IPv6 /64) may hold 256 such sockets, and the server 4,096. That is about 4 GiB in all, outside `--max-memory`, from 16 addresses or from one IPv6 /60.

Repro (`AUDIT/work/L/app1.qwe` on a server with `--max-memory 64MB` and the mimalloc caps; `p5b.py`):

    MIMALLOC_RESERVE_OS_MEMORY=256MiB MIMALLOC_ARENA_RESERVE=256MiB MIMALLOC_DISALLOW_OS_ALLOC=1 \
      quillo run app1.qwe --port 4922 --no-wt --max-memory 64MB
    python3 -I p5b.py 4922 200 <pid> 30
    # 200 sockets, one anonymous ticket; each sends ["m","x","<64 KB> as a non-final text frame + 15 x 64 KB
    # continuations (random bytes), then nothing for 30 s
    rss before KB ['quillo [75946]: 64-bit    Footprint: 6945 KB ...']
    200 rss after KB ['quillo [75946]: 64-bit    Footprint: 209 MB ...']
    server-side sockets: 400 sum Recv-Q: 0              # the server has read every byte into its buffers
    over-limit: [(8, True, b''), ('EOF', ...)]          # one more part past 1 MiB: closed, so it was counting
    after wait rss KB ['quillo [75946]: 64-bit    Footprint: 209 MB ...']

| Impact | |
|---|---|
| Functionality | A message must finish within a deadline once it has begun. Pages send small messages at once, so they do not notice. |
| User | The process is killed for memory, and every visitor loses the site. |
| Requests | 4,096 sockets and about 4 GiB of upload to take the server down. A single address takes 256 MiB. |
| Security | Denial of service with no sign-in. It needs several addresses (or one IPv6 /60) to reach the global cap. |

Fix: in `wt::read_page`, wrap each `next_async` in a deadline once a frame or a message has started (for example 10 s, as `LiveSocket::heard` does). In `ws::read_message`, read a frame's payload in pieces instead of with `vec![0; len]`. Count the bytes held in unfinished messages across all channel sockets, and refuse new sockets or close the largest past a budget.

### Q-1074. WebTransport sessions escape every limit on waiting pages, and each can hold about 100 MiB in unfinished frames

Found by auditor L (L-4).

Severity: medium. Status: PLAUSIBLE (no QUIC/WebTransport client on this machine: no aioquic, no browser allowed). Entry point: UDP on the HTTP port (`--wt-port`), path `/_quillo/wt?t=<ticket>`. Code: src/wt.rs:1283 (`tokio::spawn(session(incoming))` for every connection), src/wt.rs:1308-1380 (`session`: no `waiting_place`, no per-client count; a task per uni stream, each `vec![0u8; n]` with `n` up to 1 MiB and `read_exact` with no deadline), src/wt.rs:1242-1250 (`config`: `ServerConfig` with quinn's default transport: 100 concurrent uni streams, a 1.25 MB stream window, and an unlimited connection window).

A channel WebSocket takes one of the 4,096 waiting places and one of its client's 256 (`waiting_place`). A WebTransport session takes neither. `quic::session` accepts every connection that shows a valid ticket. A ticket costs one anonymous `POST /_quillo/wt-ticket` (verified: it also starts the UDP listener) and can be reused for 120 s. Inside a session, every uni stream gets its own task. That task reads a 4-byte length, allocates up to 1 MiB, and waits for it with no deadline. With quinn's defaults, a client can open about 100 such streams per connection and fill each almost to its end. One address can open as many QUIC connections as it likes, so memory grows by about 100 MiB per connection, outside `--max-memory`. The listener runs by default whenever `--no-wt` is not given, bound to every address unless `--host` is a loopback address.

What I verified: `POST /_quillo/wt-ticket` with no cookie returned `{"ticket":"…","port":4921,"hash":"97e5…"}`, after which `lsof` showed `UDP 127.0.0.1:4921` and the log said `quillo: WebTransport on UDP port 4921`. To confirm the rest: use an aioquic WebTransport client to open N sessions with one ticket, open 90 uni streams per session, write a length of 1048576 and 1048000 bytes on each, and watch the server's footprint.

| Impact | |
|---|---|
| Functionality | WebTransport sessions count against the same per-client and global caps as WebSockets. A page uses one session and a few streams, so it is unaffected. |
| User | The process is killed for memory. |
| Requests | A few QUIC connections from one address could take gigabytes. |
| Security | Denial of service with no sign-in, and a way around `--live-per-client`. |

Fix: in `quic::session`, take a `server::waiting_place(remote ip)` before `req.accept()` and hold it for the session. Set a `TransportConfig` with `max_concurrent_uni_streams` of a few (a page needs one per track) and `max_concurrent_bidi_streams(1)`. Give each frame read a deadline once its length has arrived, as in Q-1073.

### Q-1075. One `re.test` or `re.find` on a visitor's pattern holds an instance for seconds to minutes: Q-976's limit covers only the many-search calls

Found by auditor G (G-6).

Severity: medium. Status: CONFIRMED. Entry point: a route that runs a visitor's pattern (the case Q-975/Q-976 fixed). Code: src/re.rs:613 (`re.test`: `is_match` with no check), src/re.rs:619-620 (`re.find`: one search, and `matches` looks at the limit only every 64 searches).

Q-976 bounds `find_all`, `replace` and `split` by `--max-compute` and a scan budget. A single
search is not bounded. When the lazy DFA gives up on a pattern whose states explode
(`[ab]*a[ab]{900}c`), the regex crate falls back to its PikeVM, which takes time proportional to
pattern size times text length. Patterns of this size pass `re.valid` and the 10 MiB compile limit.
The search does not yield, so the instance's other requests wait.

Repro:
    // app.qwe (excerpt)
    route POST /re {
        let q = request.json
        if !re.valid(q.p) {
            return "bad pattern"
        }
        return str(re.test(q.text, q.p))
    }
    ./srv.sh 4972 compute app.qwe --max-memory 64MB --max-compute 2s
    # text: 200,000 random a/b; p1 = [ab]*a[ab]{900}c; p4 = four such alternatives joined by |
    curl -H 'Content-Type: application/json' --data-binary @re_p1.json localhost:4972/re   false status=200 total=2.125604s
    curl ... @re_p4.json                                                                  false status=200 total=5.232501s (again 5.852492s)
    # 1,000,000 a/b with p1-like patterns: 2.79 s, 2.91 s, 3.31 s
    # a GET / sent 0.5 s after a p4 search:
      GET / sent 0.5 s later: status=200 total=4.845411s
      log: quillo: an instance has been computing for 5 s without letting go: POST /re
           POST /re -> 200 (5354.2ms)

About 27 µs per byte with p4 means about 4.5 minutes for a 10 MB text, and the alternatives can be
widened further.

| Impact | |
|---|---|
| Functionality | Ordinary patterns are unchanged: the DFA does not give up on them |
| User | Every request on that instance waits while the search runs |
| Requests | One request per few seconds to minutes of an instance |
| Security | Availability; a bypass of Q-976 for programs that run visitors' patterns |

Fix: in `re::test` and `re::find`, for a long text, search with regex-automata's lazy DFA set to
give up (`hybrid` with `minimum_cache_clear_count`) and answer an error value, as Q-976's does,
instead of falling back to the PikeVM; or refuse when the pattern's NFA states times the text's
length passes the bound Q-976 uses (0.25 s + 4 µs a byte).

### Q-1076. Each `run_script` call can leak its source's size for good: method names are interned at run time

Found by auditor S (S-3).

Severity: medium. Status: CONFIRMED. Entry point: any route that runs a visitor's script with `run_script`. Code: src/interp.rs:1888 (`call_on`: `Sym::new(f)`), reached from src/interp.rs:3556 (`exec_assign` to `in_place_call`) and src/interp.rs:4279, 4368, 4420, 5430. Leaked at src/sym.rs:53-62 (`Box::leak`).

src/sym.rs promises that a script's names are its own, so that "the table grows to the most new names one script has, not with the scripts run". That holds while parsing. However, `call_on` runs on every assignment whose right side is a method call, and it interns the method's name with `Sym::new` at run time, outside `in_script`. The name goes into the global symbol table, which is never freed. The script `let m = 1` / `m = m.<1 MB name>()` leaks 1 MB. The call then fails with "int has no method", but the name has already been kept. With the default `--max-body` of 10 MiB, each request can leak about 10 MB.

Repro:
    (the Q-1055 server: AUDIT/work/S/srv/main.qwe; AUDIT/work/S/srv/leak3.py)
    for 30 requests: POST /rule  'let m = 1\nm.<random 1 MB name>()'        (no assignment)
    for 30 requests: POST /rule  'let m = 1\nm = m.<the same 1 MB name>()'
    for 60 requests: POST /rule  'let m = 1\nm = m.<random 1 MB name>()'

    start footprint 5281 KB
    after 30 scripts `m.<1MB name>()` (no assignment): 17 MB
    after 30 scripts `m = m.<same 1MB name>()`: 18 MB
    after 30 scripts `m = m.<distinct 1MB name>()`: 47 MB
    after 30 more: 78 MB
    (footprint of the server's PID; the memory is not given back while idle)

| Impact | |
|---|---|
| Functionality | None. The in-place paths look only for `set`, `push`, `merge`, `remove` and `get`, all of which are known names. |
| User | The server's memory grows with every hostile request until the OS kills it or `--max-memory` cannot help (the leak is not a value). |
| Requests | Leak equal to the request's size, kept for good. About 100 requests of 10 MB take 1 GB. |
| Security | Lasting memory exhaustion by any visitor of a `run_script` app. |

Fix: in `call_on`, use `Sym::known(f)` for a field callee and return `None` when the name is not known. A name no program has can be none of `set`, `push`, `merge`, `remove` or `get`.

#### Found again by auditor G (G-5): A visitor's script leaks its method names into the symbol table for good: about 1 GB a hundred requests, and a log line as long as the name

Severity: medium. Status: CONFIRMED. Entry point: any route that runs `run_script(request text, vars)` (USERGUIDE "Scripts from users"). Code: src/interp.rs:1888 (`call_on`: `Sym::new(f)` on a method name at run time), src/interp.rs:3556 / 4279 (`in_place_call` on every `name = x.f(...)`), src/sym.rs:62 (`Box::leak`).

sym.rs says a visitor's script names become `$1`, `$2`... while it is parsed, so the table does not
grow. But a method name stays text in the tree (`Expr::Field`), and `call_on`, which every
assignment `v = x.f(...)` goes through to look for `set`/`push`/`merge`/`remove`, calls `Sym::new`
on it at run time, outside `in_script`. Each new name is copied, leaked, and kept forever. The
script then fails (`int has no method ...`), but the name was already kept. That error is logged
with the whole name, so each request also writes its own size to the log.

Repro:
    // scripts.qwe
    route POST /rule = str(run_script(request.body, { value: 5 }))
    # leak.py: N scripts, each "let m = 1\nm = m.<distinct ~1 MB name>(1)\nm" (assign) or "let m = 1\nm.<name>(1)" (plain)
    ./srv.sh 4974 scripts scripts.qwe --max-memory 64MB
    python3 -I leak.py $PID5 4974 20 plain     footprint 5281 KB -> 15 MB, 20 s later 15 MB
    python3 -I leak.py $PID5 4974 20 assign    footprint 15 MB -> 35 MB, 20 s later 35 MB
    python3 -I leak.py $PID5 4974 20 plain     35 MB -> 36 MB
    python3 -I leak.py $PID5 4974 40 assign    36 MB -> 73 MB, 20 s later 73 MB
    # gzipped: 10 scripts, 11681 bytes on the wire: footprint 75 MB -> 84 MB, log +10001430 bytes
    log: quillo: error: POST /rule (...): scripts.qwe:2: line 2: int has no method `a0000391e3949d10adcbf83bbbb...` (1 MB)

Each request keeps memory equal to its decoded body: 10 MB from about 10 KB with gzip, at the
default `--max-body`. Two hundred such requests keep 2 GB and write 2 GB of log.

| Impact | |
|---|---|
| Functionality | None: a name not in the table cannot be `set`, `push`, `merge`, `remove` or `get` |
| User | The server grows until it is killed; the log fills the disk |
| Requests | About 200 small requests for 2 GB, kept until restart |
| Security | Availability, for programs that run users' scripts (the sandboxed feature made for that) |

Fix: in `call_on`, look the name up without making it: `Sym::known(f)` (None for a name the table
does not have, so the call goes the ordinary way); shorten names in the "has no method" message to
100 characters.

### Q-1077. In a mounted program, under `--base` or `--plain-http`, a sibling subdomain can plant its own valid session: the visitor is signed in as the attacker, or kept signed out

Found by auditor D (D-6).

Severity: low. Status: CONFIRMED (server side; the browser side is how browsers treat `Domain=` cookies). Entry point: the session cookie `quillo_session`. Code: src/session.rs:574 (`cookie_name`), src/session.rs:132 (`arrived_whole`).

At the root the session cookie is `__Host-quillo_session`, which a sibling subdomain cannot set. Under `--base`, `mount` and `--plain-http` it is `quillo_session` with `Path=/base`. Anyone controlling a sibling subdomain (user content, another tenant's site, a forgotten staging host) can set `quillo_session=<attacker's own valid cookie>; Domain=example.com; Path=/base`: the attacker gets it by signing in to the same program, so it is signed for the same key, mount and tenant. A visitor with no session is then signed in as the attacker (what they enter lands in the attacker's account); one who has a session has two valid cookies and so none, on every request, until they clear cookies.

Repro:
    QUILLO_SECRET=... quillo run main.qwe --port 4951 --base /app
    login as victim and as attacker -> two Set-Cookie: quillo_session=...; Path=/app; ... (no __Host-)
    Cookie: quillo_session=<attacker>               -> {"s":{"user":"attacker"}}
    Cookie: quillo_session=<victim>; quillo_session=<attacker> -> {"s":{}}

| Impact | |
|---|---|
| Functionality | The cookie name would change once (signing everyone out of mounted programs once) |
| User | Signed in as someone else without noticing, or unable to stay signed in |
| Requests | One cookie set from the sibling host |
| Security | Login CSRF / session fixation of the attacker's account, and a sign-out loop, for mounted or `--base` programs |

Fix: in `cookie_name`, when the cookie is `Secure`, use `__Host-` with `Path=/` and put the mount in the name instead of the path (`__Host-quillo_session-shop`), so `cookie_line` can keep `Path=/`; keep `quillo_session` only for plain HTTP.

### Q-1078. The worker's gate fails open under `content-security-policy: sandbox allow-scripts`: the next person on the device is shown the previous visitor's kept page offline

Found by auditor N (N-2).

Severity: low. Status: CONFIRMED (headless Chrome). Entry point: an `offline` route whose page reads
`session()` and is sent with a CSP `sandbox` that allows scripts but not `allow-same-origin` (a
page that shows content of the visitor's own in a sandbox). Code: src/client/sw.js:233-247
(`gated`), src/client/sw.js:196 (the `hidden` mark), src/client/sw-gate.js:3 (the gate).

`gated()` refuses to give back a session's kept page only for a `sandbox` without `allow-scripts`,
on the reasoning that the gate can then run. But a document sandboxed without
`allow-same-origin` has an opaque origin: the gate's first statement, `document.cookie`, throws
`SecurityError`. The gate then neither removes `hidden` nor calls `stop()`, so the rest of the
kept page is parsed and its scripts run; only the `hidden` attribute is left, which the page's own
sheet undoes (`html { display: block }`, or any `display` rule on `html`). The kept page was
written for the person signed in before; after a sign-out that the worker did not see (Q-1082),
the next person who opens that address with the server unreachable reads it.

Repro (AUDIT/work/N/swsb, Chrome through chrome.mjs, a sink on 4936 as "another site"):

    site_headers("/mine", { "content-security-policy": "sandbox allow-scripts allow-forms" })
    route GET /in/:who {
        set_session({ user: who })
        redirect("/mine")
    }
    route GET /out {
        end_session()
        redirect("http://127.0.0.1:4936/bye")
    }
    route offline GET / = <html><head><title>Home</title></head><body><h1>Home</h1></body></html>
    route offline GET /mine = <html><head><title>Mine</title><style>{"html { display: block }"}</style></head><body><h1>Account of {session().user ?? "nobody"}</h1><p>card ending 4242 of {session().user ?? "-"}</p></body></html>

    drive.mjs: open / twice (worker installed and controlling), /in/alice, then /out,
    then the server is stopped and the next person opens /mine:
    controlled: true
    alice sees: "Account of alice\n\ncard ending 4242 of alice"
    headers of /mine: content-security-policy: sandbox allow-scripts allow-forms / x-quillo-offline: private
    signed out, at: http://127.0.0.1:4936/bye
    next person sees: {"url":"http://127.0.0.1:4935/mine","text":"Account of alice\n\ncard ending 4242 of alice","hidden":true,"kept":"brEuxXfPK_dDiC5Z"}
    exception: SecurityError: Failed to read the 'cookie' property from 'Document': The document is sandboxed and lacks the 'allow-same-origin' flag.
    screenshot (swsb/next-person.png): "Account of alice / card ending 4242 of alice" on screen

| Impact | |
|---|---|
| Functionality | such a page is not given back offline (it never could be for its own session either, as the gate cannot read the cookie there) |
| User | a person on a shared device sees the previous person's account page while offline or while the server is down |
| Requests | none: the next person opens one address |
| Security | the per-session gate, the only thing between one visitor's kept page and the next person, is bypassed; the kept page's own scripts also run for that person |

Fix: in `gated()`, refuse a `sandbox` that lacks `allow-same-origin` as well as one that lacks
`allow-scripts`; and make the gate fail closed: wrap its body in `try { ... } catch (_) { stop() }`
so that any exception stops the parser before the page is read. Optionally put
`style="display:none!important"` on the worker's `<html>` tag beside `hidden` (the first `<html>`
tag's attributes win, and an inline `!important` beats the page's sheet), removed by the gate with
`hidden`.

### Q-1079. A user's own Okta access token is taken as a service caller's own token (Q-665 bypass)

Found by auditor D (D-4).

Severity: low. Status: CONFIRMED (tokens made and signed locally, JWKS served from 127.0.0.1:4953). Entry point: `POST /_quillo/service/<fn>` with `Authorization: Bearer`, `accept_calls { jwt }`. Code: src/svcauth.rs:964.

Q-665's fix refuses a token "one a user signed in for": `jwt_caller` names the caller by `client_id`, then `azp`, then `sub`, and takes the token as the client's own when `sub` equals that name. Okta's access tokens name the client in `cid` and the user in `sub` (the login), with no `client_id` or `azp`. The name falls back to `sub`, which equals itself, so every Okta user's token for this audience is a service's own, named by the user's login. A `server fn` without `for` (the guide's default) takes it; `for ["shop"]` refuses it unless a login is `shop`. A token with no `sub` at all is also taken as its `azp`'s.

Repro:
    accept_calls { jwt: { issuer: "https://idp.test", audience: "billing", jwks: "http://127.0.0.1:4953/jwks.json", secret: env("HS_SECRET") } }
    server fn whoami() { request.caller }

    claims {"iss":"https://idp.test","aud":"billing","azp":"shop","sub":"user-42",...}            -> 401 "the token is one a user signed in for" (Q-665 holds)
    claims {"iss":"https://idp.test","aud":"billing","cid":"shop","uid":"00u1","sub":"alice@example.com","scp":["billing"],...}
      -> 200 {"ok":{"name":"alice@example.com","by":"jwt","scopes":["billing"],...}}
    claims {"iss":"https://idp.test","aud":"billing","azp":"shop",...} (no sub) -> 200 {"name":"shop",...}

| Impact | |
|---|---|
| Functionality | Tokens with no client claim would be refused, or named by `cid` and checked against `sub` |
| User | An IdP user calls the service's functions as if a service |
| Requests | One request with the user's own token |
| Security | Service-only functions without `for` are open to every user of an Okta-style IdP who can get a token for that audience |

Fix: in `jwt_caller`, read the client from `client_id`, `azp` or `cid`; when none is present, refuse the token unless an app marker says it is one (`gty`, `idtyp`); and refuse a token with no `sub`.

### Q-1080. Recursive records nested past 64 levels go unchecked: wrong types, values outside a string set and extra fields reach a server fn's parameters and route patterns

Found by auditor C (C-5).

Severity: low. Status: CONFIRMED. Entry point: `POST /_quillo/rpc/<fn>` with a recursive record parameter; any route with `{ json: T }` for a recursive `T`. Code: src/record.rs:89 (`DEPTH = 64`), src/record.rs:185 (`make_at`: `depth > DEPTH` returns the value as it came), src/record.rs:99 (`fits_at`, the same).

Past depth 64, `make_at` returns the visitor's value unchanged instead of refusing it. Each level of a self-referencing record costs two (the field, then the list item), so the 33rd node of a tree is neither checked nor made. Its fields may then have any type, a `"member" | "viewer"` field may be `"owner"`, and fields the record does not have (`is_admin`) are kept. Values that type checks normally stop reach the function, which the guide says can trust its types.

Repro:

    type Node = { value: int, kids: list<Node> = [] }
    server fn tree(t: Node) { return t }
    type Team = { name: str, role: "member" | "viewer" = "viewer", sub: list<Team> = [] }
    route POST /teams { json: Team } = { accepted: json }

    leaf {"value":"NOT AN INT","kids":"not a list","is_admin":true} nested 30 deep -> rpc tree:
    # 400 ... `kids[0]...kids[0].value` must be int, got str "NOT AN INT"
    the same nested 33 deep:
    # {"ok":{... {"value":0,"kids":[{"value":"NOT AN INT","kids":"not a list","is_admin":true}]} ...}}
    POST /teams, leaf {"name":"x","role":"owner","is_admin":true,"sub":"not a list"}, 31 deep: 400
    the same 33 deep:
    # {"accepted":{... "role":"owner","is_admin":true,"sub":"not a list" ...}}

| Impact | |
|---|---|
| Functionality | A value nested deeper than the limit would be refused (400 / no match) rather than taken unchecked |
| User | A program that stores or acts on a nested record (a comment thread, an org chart, a menu) gets a node of the wrong shape |
| Requests | One request of a few KB |
| Security | Bypasses the type and field filtering (extra fields such as `is_admin`, set membership) that programs rely on for nested input |

Fix: in `record::make_at` and `record::fits_at`, return `Err(Why { what: "is nested too deep" })` when `depth > DEPTH`, not `Ok`.

### Q-1081. The app passes every visitor header to a split service, the root site's `__Host-quillo_session` and `Authorization` included, at whatever address `--service-url` names

Found by auditor C (C-7).

Severity: low. Status: CONFIRMED. Entry point: any route under a split service's prefix. Code: src/service.rs:2323-2333 (`HOP` drops only hop headers and `proxy-authorization`).

`pass_on` copies all request headers except hop-by-hop ones. The service therefore receives the parent site's session cookie, which the browser sends to `/shop/*` because it is `Path=/`, and every other credential the visitor holds for the whole site. A service never needs the root's session, which is signed for another mount and refused there. If a service runs on another host (`--service-url shop=http://10.0.0.5:8081`, plain HTTP by default), every visitor's root session crosses that network and is handed to that process. The service's operator, or anyone who compromises it or watches that link, can replay the cookies against the root app.

Repro:

    (split app above, QUILLO_SECRET set)
    R=<root's __Host-quillo_session from GET /setc>
    curl 127.0.0.1:4911/shop/sess -H "Cookie: __Host-quillo_session=$R"
    # {"s":{},"cookies":{"__Host-quillo_session":"eyJlIjoxNzk3NTkzNDM4LCJkIjp7InVzZXIiOiJyb290dXNlciJ9...
    (the service process received the root user's session cookie whole)

| Impact | |
|---|---|
| Functionality | The root's own session cookie (and optionally cookies scoped to other mounts) would be removed from the `Cookie` line passed on; the service's own `quillo_session` and other cookies stay |
| User | A less-trusted or compromised service, or a sniffed internal link, gets sessions for the main site |
| Requests | None |
| Security | Lateral movement from one service to every user's main-site account |

Fix: in `service::pass_on`, rewrite the `Cookie` header without the parent's session cookie name (`__Host-quillo_session` / `quillo_session` of the root mount). Consider a per-service `forward` allowlist for credential headers, as signed calls already have.

### Q-1082. A sign-out that redirects to another origin leaves the signed-out person's kept pages in Cache Storage, readable by the next person on the device, until someone opens a page of the site online

Found by auditor N (N-4).

Severity: low. Status: CONFIRMED (headless Chrome, Cache Storage read over the DevTools protocol, as
the Application panel does). Entry point: `end_session()` followed by `redirect("https://idp.example/logout")`
(the common SSO sign-out), or any sign-out the worker does not see. Code: src/client/sw.js:52-55 and
66-90: a navigation's redirect comes back to the worker as an opaque redirect, whose
`x-quillo-session` it cannot read, and the next page is on another origin; only
`current()`/`drop()` (sw.js:142-163) ever delete a session's cache.

The guide accepts the delay ("A sign-out the worker could not see (a redirect to another site) is
caught so, at the next page of the site"), and the gate keeps the copy from being shown (but see
Q-1078). On a shared or kiosk device, though, the copy stays on disk with the person's data after
their session cookie and `quillo_tag` are gone, and anyone with DevTools (Application, Cache
Storage) reads it, with no need for the network to fail.

Repro (AUDIT/work/N/swc):

    route GET /in/:who {
        set_session({ user: who })
        redirect("/mine")
    }
    route GET /out {
        end_session()
        redirect("http://127.0.0.1:4936/bye")
    }
    route offline GET / = <html><head><title>Home</title></head><body><h1>Home</h1><a href="/mine">mine</a></body></html>
    route offline GET /mine = <html><head><title>Mine</title></head><body><h1>Account of {session().user ?? "nobody"}</h1><p>card ending 4242 of {session().user ?? "-"}</p></body></html>

    drive.mjs: / twice, /in/alice, /out (to the other origin), then /_quillo/x (an address the
    worker leaves alone) to read the site's Cache Storage:
    caches after visit: quillo::3101f592b99213b7:aSAqMZw1U7Epaa58 => /mine
    after logout at: http://127.0.0.1:4936/bye
    caches seen on the site's origin after logout:
      'quillo::3101f592b99213b7:aSAqMZw1U7Epaa58 => /mine'
      body of the kept /mine: <!doctype html><html><head><title>Mine</title></head><body><h1>Account of alice</h1><p>card ending 4242 of alice</p>...
    (opening /mine with the server down then shows the offline page and drops it: the gate works)

| Impact | |
|---|---|
| Functionality | the worker drops a session's pages as soon as the browser's `quillo_tag` changes, wherever the sign-out answer went |
| User | on a shared device, the next person can read the last person's kept pages (orders, account details) in DevTools |
| Requests | none |
| Security | data of a signed-out session stays at rest on the device for an unbounded time |

Fix: let the worker follow the cookie itself: in `activate`, `self.registration.cookies?.subscribe([{
name: "quillo_tag", url: BASE + "/" }])`, and on `cookiechange` call `current(<the new value or
"-">, true)` (Chromium's Cookie Store API in workers); elsewhere, after each navigation `fetch` in
`page()` and in the non-GET branch, read `cookieStore.get("quillo_tag")` where it exists and call
`current()` with it, so an opaque redirect still drops the old session's pages.

### Q-1083. A URL put into a translation's `href` by `raw(t(...))` is not checked for `javascript:`, where markup blocks it

Found by auditor F (F-5).

Severity: low. Status: CONFIRMED. Entry point: a translation with a link placeholder fed with user data. Code: src/i18n.rs:422-425 (values are HTML-escaped only), compare the markup attribute check (`interp::script_url`).

`<a href={site}>` in markup turns `javascript:...` into `about:blank#blocked`, but the same value in a translation `"Posted by <a href=\"{site}\">{name}</a>"` used through `raw(t(...))` is only HTML-escaped, which does nothing to a scheme. A user's profile website shown through such a translation runs script when another user clicks it.

Repro:

    translations({ en: { by: "Posted by <a href=\"{site}\">{name}</a>" } })
    route GET /by { query: { site: str, name: str } } = <p>{raw(t("by", { site: site, name: name }))} | <a href={site}>{name}</a></p>

    $ curl 'http://127.0.0.1:4960/by?site=javascript:alert(document.cookie)&name=%3Cb%3EAda%3C/b%3E'
    <p>Posted by <a href="javascript:alert(document.cookie)">&lt;b&gt;Ada&lt;/b&gt;</a> | <a href="about:blank#blocked">&lt;b&gt;Ada&lt;/b&gt;</a></p>

| Impact | |
|---|---|
| Functionality | A value that is a `javascript:`/`vbscript:` URL put into a translation's HTML form shows as `about:blank#blocked`, as in markup. |
| User | A click on another user's link runs their script in the victim's session. |
| Requests | One stored value and one click. |
| Security | Stored XSS through a common i18n pattern (links in translated sentences), only where the URL comes from users. |

Fix: in `i18n::translate` with `html` (and runtime.js `translate` with `B.escape`), pass each value through `script_url` as markup attributes do, putting `BLOCKED_URL` in place of a script URL, before escaping it.

### Q-1084. `quillo check` (Q-965) is silent on GET routes that surely write through `http.stream`, `udp.send`, `http.request` with its method in a `let`, a program function called through a local name, or SQL held in a `let`

Found by auditor N (N-3).

Severity: low. Status: CONFIRMED. Entry point: `quillo check`, the LSP. Code: src/check.rs:10563-10625
(`GetWalk::call`: `http.stream` and `udp.*` are not looked at, `http.request` only with a string
literal method, a call through a name `let` binds to a program function is `Bound::Plain` and not
followed), src/check.rs:10680-10700 (`GetWalk::db`: SQL only when written in the call).

Q-965's warning is the only defence against another site's link running a GET that writes as the
visitor (SameSite=Lax cookies go with a followed link). It is said "where the text makes both
certain"; each of these is certain from the text. The first three are also not refused at run time
(Q-1062); the last two are refused for a prefetch by the database layer, but not for a followed link.

Repro (AUDIT/work/N/check2):

    db.open("sqlite:app.db")
    fn save_visit(u) { db.exec("insert into visits (user) values (?)", u) }
    route GET /a with http {
        http.stream("https://api.example.com/charge", (d) => print(d), { method: "POST", body: { who: session().user } })
        "ok"
    }
    route GET /b with http {
        let m = "POST"
        http.request(m, "https://api.example.com/charge", { who: session().user })
        "ok"
    }
    route GET /c with net {
        udp.send("10.0.0.5", 514, "vote " + str(session().user))
        "ok"
    }
    route GET /d {
        let w = save_visit
        w(session().user)
        "ok"
    }
    route GET /e {
        let q = "insert into visits (user) values (?)"
        db.query(q, session().user)
        "ok"
    }
    route GET /f {
        db.exec("insert into visits (user) values (?)", session().user)
        "ok"
    }

    $ quillo check main.qwe
    main.qwe:26: warning: `GET /f` writes (`db.exec("insert into visits (user) values (?)")`) and reads who the visitor is (`session()`): ...
    main.qwe: ok (6 routes, 1 function, 0 components), 1 warning

Only the control `/f` is warned of. (A list of functions, `let fs = [save_visit]; fs[0](...)`, is
silent too.)

| Impact | |
|---|---|
| Functionality | five more shapes of a sure write are warned of (a `let` of a string literal is followed as the literal; a `let` of a program function as that function; `http.stream`'s `method`/`body` options and `udp.send` are writes) |
| User | a developer who relies on the check ships a GET that another site's link runs with the visitor's session |
| Requests | one link followed |
| Security | login-CSRF-style writes by GET (Q-965) pass the check that 0.8.7 added for them |

Fix: in `GetWalk::call`, treat `http.stream` with a `method` other than GET or a `body` option,
`udp.send`/`udp.reply` and `channel.send`/`broadcast` as writes; take `http.request`'s method from a
`let` bound to a string literal, and a non-literal method as a write; bind `let x = <program fn>`
as that function (`Bound::Held` or the name pushed to `calls`); and in `GetWalk::db`, read the SQL
of a `let` bound to a string literal or template.

### Q-1085. `quillo check` says nothing about SQL injection built in a helper function or through `str`, `trim`, `lower`, `replace`, a map field or a joined list variable

Found by auditor D (D-5).

Severity: low. Status: CONFIRMED. Entry point: `quillo check`, the LSP. Code: src/check.rs:8315 (`sql_built`).

`sql_built` follows templates, `+`, `join([...])` written in place, and `let` variables. It does not follow the value back through a call: `db.query(notes_of(owner))` where `fn notes_of(o) = "... '" + o + "'"`, a common way to keep SQL together, is silent, and so are `str(...)`, `trim(...)`, `lower(...)`, `replace(...)`/`.replace(...)`, `` `...${x}` | trim ``, a field of a map holding built SQL, a lambda that builds it, and `join(parts, "")` of a list kept in a variable. The guide promises a warning for SQL "made first in a variable". No visitor can switch the warning off; the gap is what a developer is not told.

Repro:
    fn notes_of(owner: str) -> str { return "select body from notes where owner = '" + owner + "'" }
    route GET /notes = db.query(notes_of(request.query.owner ?? "bob"))

    quillo check helper.qwe        -> helper.qwe: ok (1 route, 1 function, 0 components)
    curl '.../notes?owner=bob'     -> [{"body":"bob note"}]
    curl --get --data-urlencode "owner=x' or '1'='1" .../notes
                                   -> [{"body":"alice private note"},{"body":"bob note"}]
    work/D/chk/main.qwe: of 17 injectable shapes, 9 are silent: lines 6 (helper fn), 8 (str), 10 (.replace),
    11 (replace), 14 (join of a list variable), 18 (map field), 33 (lower), 37 (lambda), 41 (template | trim);
    warned: 4, 5, 7, 9, 23, 27, 34 (sql`...`), 40; line 31 (sql_name) rightly silent

| Impact | |
|---|---|
| Functionality | More warnings, on code that builds SQL through calls (none on `sql_name`/`sql_param` code) |
| User | A developer trusting a clean `quillo check` ships an injectable query |
| Requests | n/a |
| Security | SQL injection in programs that build SQL in helpers |

Fix: in `sql_built`, treat a call of a program function whose body returns built SQL (the counterpart of `sql_name_fns`) as built, and treat `str`, `trim`, `lower`, `upper`, `replace` (function and method, piped too) as passing their first argument's built-ness through; follow `Expr::Field` of a map literal variable.

### Q-1086. Server fn calls tell any caller internals: a failing record default's file, line and error (outside `--watch`), and a `for [...]` function's caller services

Found by auditor C (C-8).

Severity: low. Status: CONFIRMED. Entry point: `POST /_quillo/rpc/<fn>`. Code: src/server.rs:2891 (`take_args` formats `Ctrl::Error` into the 400), src/server.rs:1976.

A server fn's own runtime error is hidden from visitors ("internal error in `f`") unless `--watch` is on. A runtime error raised while its arguments are made (a record default that fails) is instead formatted into the 400 with its file, line and message. Calling a `for [...]` server fn from a page also names the internal services allowed to call it.

Repro:

    fn fee() -> int {
        let key = env("SECRET_API_KEY") ?? "none"
        return 100 - key
    }
    type Fee = { amount: int, fee: int = fee() }
    server fn with_fee(f: Fee) { return f }
    server fn admin_only() for ["billing"] { return "secret" }

    curl 127.0.0.1:4910/_quillo/rpc/with_fee --json '[{"amount":5}]'
    # {"error":"argument `f` of `with_fee`: main.qwe:65: cannot subtract int and str"}
    curl 127.0.0.1:4910/_quillo/rpc/admin_only --json '[]'
    # {"error":"no server fn `admin_only` for pages: it takes calls from billing only"}

| Impact | |
|---|---|
| Functionality | Such failures would answer 500 `internal error in f` (full text under `--watch` and in the log), as a body's failure does; the `for` refusal would be a plain 404 |
| User | Source file names, line numbers and error text reach any visitor |
| Requests | One request |
| Security | Small information leak (program layout, internal service names) that helps further probing |

Fix: in `server::take_args`, log `Ctrl::Error` with `error_line` and return `rpc_failure(def.name, &e)` with status 500 instead of the formatted text. Make the `fn_callers` refusal the same `no server fn` 404 as for an unknown name.

### Q-1087. A program's `site_headers` (CSP, HSTS, Referrer-Policy) never reach the answers of a program it mounts, or of a service it serves under a prefix

Found by auditor C (C-6).

Severity: low. Status: CONFIRMED. Entry point: any path under a `mount` or a prefixed `service` (in one process or `--split`). Code: src/server.rs:606-646 (`handle` returns the mount's or the remote service's answer before src/server.rs:745 `site_headers_into` runs).

The guide says `site_headers(map)` at the top level "adds headers to every answer that does not set them itself". The answers of mounted programs and prefixed services skip the parent's rules entirely: they carry only their own program's headers (and Quillo's `x-frame-options` default). An app that sets its CSP and HSTS once in `app.qwe` has none of it on `/shop/*` and `/notes/*`. If XSS appears in a mounted part, the CSP the developer set does not limit it.

Repro:

    // split/app.qwe
    site_headers({ "content-security-policy": "default-src 'self'", "strict-transport-security": "max-age=31536000", "referrer-policy": "same-origin" })
    service /shop = "shop.qwe"
    mount /notes = "notes.qwe"

    quillo run app.qwe --port 4911 --split      (and without --split: the same)
    curl -i 127.0.0.1:4911/          # content-security-policy, strict-transport-security, referrer-policy, x-frame-options
    curl -i 127.0.0.1:4911/shop/     # x-frame-options only
    curl -i 127.0.0.1:4911/notes/    # x-frame-options only

| Impact | |
|---|---|
| Functionality | The parent's `site_headers` would apply under its mounts as under its own paths (the mount's own rules and the route's headers winning) |
| User | Security headers the developer set site-wide are missing on part of the site |
| Requests | None |
| Security | Weakens defence in depth (CSP against XSS, HSTS on a first visit to a mounted path) |

Fix: in `Interp::handle`, run the parent's `site_headers_into(path, ...)` on the answer returned from `sub.handle` and from `service::pass_on`, adding only headers the answer lacks. Otherwise, document that each mounted program needs its own.

### Q-1088. `quillo serve DIR` (and `files(".", path)`) serves the folder's own `quillo.policy`

Found by auditor F (F-6).

Severity: low. Status: CONFIRMED. Entry point: GET /quillo.policy. Code: src/builtins.rs:2202-2211 (`hidden_path` hides dot names and Quillo's sources, not the policy), src/main.rs:395-396 (`quillo serve` is `files(".", path)` with DIR as the program's folder, so DIR/quillo.policy is the policy it reads).

`quillo serve` reads `DIR/quillo.policy` as its policy and then serves it to anyone. A policy lists the names of the environment variables the program may read (`[env] names`), the internal hosts it calls (`[http] hosts`), its plugins and its limits: a map of the deployment for an attacker, not secrets themselves.

Repro:

    $ ls -a site/    ->  .env .env.production .git data.db index.html lib.qwe quillo.policy ...
    $ quillo serve . --port 4961
    quillo: policy ./quillo.policy
    /.env  404   /.git/config  404   /lib.qwe  404   /data.db  404
    /quillo.policy               200 application/octet-stream

| Impact | |
|---|---|
| Functionality | `quillo.policy` (and, for a program's own folder, `plugins.lock` and `app.toml` could be added) is a miss like `.env`. |
| User | None directly. |
| Requests | One. |
| Security | Names of secrets, internal hosts and limits leak to anyone. |

Fix: in `hidden_path`, keep back a last part that folds to `quillo.policy` (the `PROGRAM_FILE`), as `.qwe` is kept back.

### Q-1089. `http.*` lets a program's headers set `content-length` against the body it sends: a program that passes a visitor's header names on lets them smuggle a second request to the upstream

Found by auditor N (N-5).

Severity: low. Status: CONFIRMED (the mechanism, on a local keep-alive upstream); PLAUSIBLE as an
attack, as it needs a program that forwards header names a visitor chose (a proxying route).
Entry point: `http.request(method, url, body, headers)`, `http.post(url, body, { headers })` and the
like. Code: src/fetch.rs:396-398 (`call`: each program header `req.set(k, v)`, `content-length` and
`transfer-encoding` among them) and src/fetch.rs:404-409 (the body sent whole after it).

Q-972 refuses a method that is not a token, and ureq refuses CR/LF in header names and values (all
my tries in AUDIT/work/N/http1/main.qwe were refused: `Bad Header`). But a well-formed
`content-length` header is passed through and the body is then written in full, so the upstream
reads the rest of the body as the next request on that connection. A program that copies headers it
was sent (a small API gateway: `http.request(request.method, upstream + path, request.body,
request.headers)`, or JSON `{ headers }` from a client) lets a visitor send any request, with any
`Host`, to the upstream, past whatever paths the program allows.

Repro (AUDIT/work/N/http1; ka.py is a raw HTTP/1.1 keep-alive server on 4932 that answers each
request it parses by `Content-Length` and logs it):

    let u = "http://127.0.0.1:4932"
    let a = http.request("POST", u + "/first", "abcGET /smuggled HTTP/1.1\r\nHost: x\r\n\r\n", { "content-length": "3", "content-type": "text/plain" })
    print("A got: " + str(a.body))

    $ quillo run smug.qwe
    A got: answer to POST /first HTTP/1.1
    ka.log:
    conn1: 'POST /first HTTP/1.1' body=b'abc'
    conn1: 'GET /smuggled HTTP/1.1' body=b''

The upstream ran `GET /smuggled` with `Host: x`, which the program never asked for. (`transfer-encoding:
chunked` given the same way failed locally with `os error 22`, so it is not a second way.)

| Impact | |
|---|---|
| Functionality | `content-length`, `transfer-encoding`, `connection`, `te`, `trailer`, `upgrade` (and `host`, which ureq sets from the URL) in a program's headers are an error value, or dropped, as `fetch` forbids them in a browser |
| User | nothing for a program with fixed header names |
| Requests | one visitor request through a proxying route |
| Security | a request of the attacker's choosing to the upstream (an internal API), from the server's address |

Fix: in `fetch::call` (and `push::post`, `service` calls), drop or refuse headers named
`content-length`, `transfer-encoding`, `connection`, `keep-alive`, `te`, `trailer`, `upgrade` and
`host` from the program's map before `req.set`, as the Fetch standard's forbidden request headers
are, and let ureq write the length of the body it sends.

### Q-1090. gzip request bodies have no ratio limit: a 10 KB request carries 10 MB, on every path, and every size limit after it sees only the decoded size

Found by auditor G (G-10).

Severity: low. Status: CONFIRMED. Entry point: any request with `Content-Encoding: gzip` (or `x-gzip`). Code: src/httpd.rs:2490-2510.

`--max-body` bounds what the gzip decodes to, so this is no bomb by itself. Decoding 10 MB of
zeros takes about 2 ms on the event loop. But the decoded body is what the route, the JSON reader,
the markdown door's 1 MB check and the arguments' readers see, so every per-byte cost above is
reachable at about 1/1000 of the bytes on the wire. `deflate`, `br` and stacked codings are
refused (415).

Repro:
    python3 -I -c "import gzip; open('b_zero10m.gz','wb').write(gzip.compress(b'\0'*(10*1024*1024-16),9))"   # 10,221 bytes
    50 x POST /no-such-path with it: 0.11 s, last status 404 (server CPU 5.35 s -> 5.45 s)
    11 MB decoded (11,237 bytes): 413 request body too large (once decompressed)

| Impact | |
|---|---|
| Functionality | A real compressed upload (ratios of 3-20x) is unaffected by a ratio cap of, say, 100x |
| User | None directly; it is the multiplier for Q-1052, Q-1057, Q-1069, Q-1076 and Q-1070 |
| Requests | 10 MB of server work for each 10 KB sent |
| Security | Availability: it removes the bandwidth cost that `--max-body` puts on those attacks |

Fix: in `parse`, refuse a gzip body whose decoded size passes 100 times its compressed size plus
64 KB (413), or decode only up to a separate, smaller limit unless the program asks for more.

### Q-1091. With no `--drain`, a body trickled at more than 1 KiB/s holds a graceful stop for as long as it keeps coming (2.8 h for 10 MB)

Found by auditor G (G-9).

Severity: low. Status: CONFIRMED. Entry point: any connection, during `SIGTERM`. Code: src/httpd.rs:1298-1311 (`STOPPING`: `close_idle` and `quiet()` keep a connection that is still receiving), src/httpd.rs:1416 (a body is given up only past 30 s of silence or under 1 KiB/s).

The stop is meant to wait for every request in progress, with no limit by default (the user's
decision, 2026-11-09). A request whose body is still arriving has not started. Its sender can
stretch it to `--max-body` / 1 KiB/s: 10,240 s at the default. Meanwhile the old process neither
serves (its listener is closed) nor exits. With systemd, `TimeoutStopSec` ends the wait with a
SIGKILL that also cuts every other request still in flight.

Repro:
    ./srv.sh 4973 stop app.qwe --max-memory 64MB
    python3 -I stopwait.py $PID4 4973    # head with Content-Length: 10000000, then 205 bytes every 0.1 s
      SIGTERM at 3.1s
      trickle stopped at 48.0s; server alive: True
      server alive=False at 81.0s (77.9s after SIGTERM)
      connection told: b'HTTP/1.1 408 Request Timeout'
    log: quillo: stopping once the requests in progress are answered (again to stop now)
    # with --drain 5s: server exited by 5.2s after SIGTERM

| Impact | |
|---|---|
| Functionality | An upload that started before the stop would get 408/503 when the stop comes, unless `--drain` is long |
| User | A deploy or restart waits for hours, or ends in a SIGKILL that cuts other visitors' requests |
| Requests | One connection sending 1-2 KiB/s |
| Security | Availability of deploys; a second signal or `--drain` works around it |

Fix: in `Loop::run`'s stopping branch, give a connection whose request has not fully arrived
(`c.receiving.is_some()`) a short deadline from the stop (a few seconds, then 503 with
`Connection: close`), and keep waiting without limit only for requests being answered.

### Q-1092. Crafted numbers make `stats.mode` and `stats.multimode` quadratic: 3 MB of JSON holds an instance for 3.5 s (about 40 s at 10 MB), past `--max-compute`

Found by auditor G (G-8).

Severity: low. Status: CONFIRMED. Entry point: a route that passes a visitor's list (mixed types, or up to 16,384 numbers) to `stats.mode`/`stats.multimode`. Code: src/numeric.rs:1414-1430 (`Bits` hasher, fixed, no key), src/numeric.rs:1434 (`tally`).

`tally` counts numbers in a `HashMap` with a fixed multiply hash: `((x ^ x>>29) * K) ^ (x>>32)`.
For any choice of bits 24..51 of an f64, its low 24 bits can be solved so that every hash has the
same low 24 bits. All the keys then probe the same buckets. Lists of more than 16,384 numbers go a
sorted way, but one text in the list sends any length to `tally`. Every request map in Quillo is
SipHash-keyed; this one is not. Its comment says "the worst is a slow mode".

Repro:
    // app.qwe: route POST /mode = str(stats.mode(request.json ?? [0]))
    python3 -I hashflood.py N hfN      # N floats in [1,2) with h & 0xFFFFFF == 0, + "x"; and N random ones
    ./srv.sh 4972 compute app.qwe --max-memory 64MB --max-compute 2s
      n=10000  rand 0.001292s   coll 0.011424s
      n=20000  rand 0.001644s   coll 0.045033s
      n=40000  rand 0.002781s   coll 0.285329s
      n=160000 rand 0.013919s   coll 3.525928s   (3 MB body; status 200 despite --max-compute 2s)

| Impact | |
|---|---|
| Functionality | None |
| User | The instance stops answering for the duration |
| Requests | One request of 10 MB: about 40 s of an instance |
| Security | Availability, for programs that run `stats.mode` on visitor data |

Fix: in `tally`, hash numbers with a process-keyed hasher (`RandomState`, or `unique_of`'s keyed
`mix`).

### Q-1093. `public_only` and `[http] private = false` take `::ffff:0:a.b.c.d` (IPv4-translated, RFC 2765/6145) for a public address

Found by auditor F (F-7).

Severity: low. Status: PLAUSIBLE (the check passes, confirmed; reaching an inside address needs a stateless SIIT translator on the host's network, which this Mac has not). Entry point: `http.*` with `public_only: true`, or any call under `private = false`. Code: src/fetch.rs:270-296 (`public_ip`: `::ffff:0:0/96` is neither `to_ipv4_mapped` nor all-zero in `s[0..6]`); src/policy.rs:466-483 (`private_ip` relies on it).

`[::ffff:0:127.0.0.1]` passes both checks and is connected to; on this Mac it fails with "No route to host". On a network whose translator serves `::ffff:0:0/96` (SIIT, as 64:ff9b:1::/48 was treated in FEEDBACK67), it reaches the IPv4 address it embeds, loopback or metadata included.

Repro:

    route GET /pub { query: { u: str } } { let r = http.get(u, {}, { public_only: true }) ... }
    $ curl 'http://127.0.0.1:4965/pub?u=http%3A%2F%2F%5B%3A%3Affff%3A0%3A127.0.0.1%5D%3A4964%2Fx'
    server.log: http: http://[::ffff:0:7f00:1]:4964/x: Connection Failed: Connect error: No route to host (os error 65)
    (every other form, [::ffff:127.0.0.1], [::127.0.0.1], [64:ff9b::127.0.0.1], [2002:7f00:1::], 0x7f.1, ... : "refused by public_only")

| Impact | |
|---|---|
| Functionality | None: the range is not on the public internet. |
| User | None on a usual host. |
| Requests | One. |
| Security | SSRF past `public_only` only where a SIIT translator exists. |

Fix: in `fetch::public_ip`, treat `::ffff:0:0/96` (segments `[0,0,0,0,0xffff,0,..]`) as its embedded IPv4 address, as NAT64 and 6to4 are.

### Q-1094. One UDP sender of empty (or tiny) packets fills a port's queue, is never held back by the per-sender rule, and makes the server refuse every other sender's packets

Found by auditor L (L-8).

Severity: low. Status: CONFIRMED. Entry point: any `udp.listen` port. Code: src/udp.rs:106-121 (`Queue::admit`: a sender's share is measured in bytes, `mine * 2 > self.bytes`, while the queue fills by packet count).

The 0.7 rule ("a sender holding more than half of it while others wait has its new packet dropped, so one sender cannot push out the rest") counts each sender's bytes. An empty datagram counts 0 bytes, so its sender never holds "more than half" of anything. When its packets fill the queue's packet cap, it pushes out the oldest packets, the legitimate ones included. Worse, a legitimate sender's next packet arrives when that sender holds 100% of the queued bytes, so the rule drops it. 1-byte packets do almost as well. No spoofing is needed: one address is enough.

Repro (`AUDIT/work/L/app4.qwe`; the attacker on 127.0.0.1, the sensor on ::1, each sending for 3 s):

    udp.listen(4925, (p) => {
        sleep(2)                                // a database write per reading
        if p.text != null && p.text != "" { got = got + [p.text] }
    }, { host: "::", queue: 1000 })
    route GET /got = got

    quillo run app4.qwe --port 4926
    python3 -I p7_udp.py none ; python3 -I p7_udp.py onebyte ; python3 -I p7_udp.py empty
    none: attacker sent 0 packets; legit sent 224, handler got 224 of them
    onebyte: attacker sent 142000 packets; legit sent 71, handler got 41 of them
    empty: attacker sent 270200 packets; legit sent 135, handler got 2 of them

| Impact | |
|---|---|
| Functionality | A sender's share is measured in packets as well as bytes (or each packet costs at least a minimum), so empty packets count. |
| User | A sensor or a game client stops being heard while one sender floods the port. |
| Requests | A stream of empty datagrams faster than the handler. |
| Security | Denial of a UDP feature from one address, defeating a shipped protection (Q-604, 0.7). |

Fix: in `Queue::admit`, track each sender's packet count next to its bytes. Treat a sender as holding more than half when either its packets or its bytes (counting each packet as at least, say, 64 bytes) exceed half of the queue's.

### Q-1095. `check_program` repeats text from `.rs` files in the program's folder

Found by auditor S (S-4).

Severity: low. Status: CONFIRMED. Entry point: `check_program` with user code that says `use some.rs`. Code: src/check.rs:2730-2739 (the `parsed.problems` of `crate::native::parse` are reported for a `.rs` file read in a confined check).

The guide says a confined check's "messages never show a file's text", and Q-703 stopped `use .env` from saying values back. For a `.rs` file, which a confined check may read for signatures, a parameter or result type that cannot cross is quoted verbatim. That quote runs from the `:` to the next `,` or `)`, across lines. Most `.rs` files hold no secrets in signatures, so the leak is narrow. Still, it is file text going to the person whose code is checked.

Repro:
    # AUDIT/work/S/chk/k2.rs
    pub fn leak(a: DATABASE_URL=postgres://admin:hunter2@db.internal/prod
    STRIPE_KEY=sk_live_51HxxSECRET
    ) -> u8 { 1 }
    pub fn r(a: i64) -> AWS_SECRET_ACCESS_KEY_wJalrXUtnFEMI { 1 }

    # user code checked: `use k2.rs`, via check_program("user.qwe", { "user.qwe": code })
    { file: "k2.rs", line: 1, severity: "warning", message: "`leak` is not exported: `DATABASE_URL=postgres: STRIPE_KEY=sk_live_51HxxSECRET` cannot cross between Rust and Quillo (...)" }
    { file: "k2.rs", line: 4, severity: "warning", message: "`r` is not exported: `AWS_SECRET_ACCESS_KEY_wJalrXUtnFEMI` cannot cross between Rust and Quillo (...)" }

| Impact | |
|---|---|
| Functionality | A confined check says "`leak` is not exported: a parameter's type cannot cross" without quoting the type. |
| User | None in a normal setup. |
| Requests | One request. |
| Security | Fragments of Rust sources in the program's folder are disclosed to the person whose code is checked. |

Fix: in the `.rs` branch of the `use` check (check.rs:2735), when `self.confined` is set, report the native parser's problems without their quoted text, or not at all.

## Checked and sound, and not covered, by group

Each auditor's own lists, as written: what was tried and held, and what was left out. The main session's notes are marked as such.

### Group C: server functions, page state, internal paths

#### Checked and sound

- **rpc argument types:** int, str, bool and list parameters refuse other JSON types. `{"$quillo_err":...}` given to a typed parameter is refused. Given to an untyped one, it arrives as a hidden (not the program's own) error, which the answer shows as `internal error`.
- **Q-640 still holds:** `{"$site","$url"}` maps sent to a server fn, plain or in the `{"$":[k,v]}` wrapper, make no outbound request (`map has no method notes`; my listener on 4917 got 0 bytes).
- **`$dec` and `$int` edge values:** `$dec` with `NaN`, `1e1000`, `1e100000000` or 5,000 digits is refused. An `$int` overflow gives a 500 `internal error`, no crash.
- **Records within 64 levels:** fields the record lacks (`is_admin`) are dropped, and values outside a string set are refused with 400.
- **Q-641 still holds:** a `for [...]` server fn gives 404 to pages. An in-process service's server fns (`/_quillo/rpc/invoice`, `billing.invoice`, `billing%2Einvoice`, `/pay/_quillo/rpc/*`) are refused to pages.
- **rpc and markdown gates:** `Sec-Fetch-Site: cross-site` or `same-site`, and `Origin: null` or another host, give 403. `text/plain` and multipart without `X-Quillo-Rpc` give 415. GET gives 405.
- **Form gate:** cross-site, same-site, `Origin: null`, another origin, an empty content type and `text/plain;charset=x` all give 403. A same-origin post gives 200.
- **Page state isolation:** across 60 concurrent requests from 5 visitors, with sleeps, `parallel` and a set in between, each got only its own value. 40 concurrent pages with two deferred parts each got their own session and page state in every part. Views that read a page state are not cached across visitors. A page state has no server-side key or store to guess or fill: it is per request, by request number on the loop's thread, and removed when the task ends.
- **Deferred parts:** they come in the same streamed answer. There is no separate id or endpoint another visitor could fetch.
- **Idempotency:** a key's row is scoped by tenant and mount. Another session, an anonymous caller, an added `Authorization` or two session cookies give 409 for a route and for a server fn. A replay between two anonymous callers happens, as documented.
- **Sessions across mounts and services:** the root's, a mount's and a split service's session cookies are each refused by the other two programs.
- **Internal paths without credentials:** `later`, `tick`, `job`, `channel` and `udp` give 404 (the token is compared in constant time). `join`, `service/*`, `service-channels` and `channel-relay` give 401 or 403. `events`, `dev`, `metrics` and `describe` give 404 without their flags. `health/queries` hides statements without the token and refuses POST. `quillo.jsx` gives 404. `/_quillo/src/<h>/..%2f..` gives 404. `announce` is HMAC-checked with a replay cache.
- **`pass_on`:** `NOT_PASSED_ON` blocks `rpc`, `service`, `job` and the rest. `//_quillo//rpc` and `rpc%2F` variants never reach a server fn, since a service refuses unsigned rpc. A service's MCP door gives 404. The visitor's own `X-Forwarded-For`, `-Host` and `-Prefix` are overridden by the app's (the last header wins in `request.headers`). The scheme word and the trusted-proxy mark are stripped.
- **`/_quillo/markdown` cost:** 0.9 MB of code blocks, nested emphasis, brackets or quotes is answered in under 70 ms. `{html:true}` affects only the caller's own text.
- **Bundle guard (Q-1063's working part):** constants from `env`, `read_file` or `db`, through program functions or a `use`d module, stop the load.
- **Errors in answers:** builtin errors held in a value are hidden from visitors, including inside the type-mismatch text (`got error err("internal error")`).

#### Not covered

- **Other auditors' areas:** live, channels, ws/wt, udp, push, the service worker and MCP. Only their exposure through `pass_on` was looked at.
- **Wait-pool exhaustion through passed-on long polls and streams:** four held long polls with `--workers 4` did not block other routes. Not pursued further.
- **A plugin-sourced constant (Q-1063):** the catalog's prebuilt dotenv library was built by an older Quillo, and loading it would need cargo, so this path is traced only.
- **The rpc `x-quillo-live` shared-run and patch cache:** read (FEEDBACK55 reviewed it) but not probed. Note that an acted-but-not-personal run's text is kept under `everyone`.
- **Tenant-by-subdomain behind a split prefixed service:** the service sees its own `Host`, so the tenant is not found. This fails closed (traced, not probed).
- **DNS rebinding in production:** `Host` is checked only under `--watch`, so an intranet server answers a rebinding page's server-fn and form calls (without the victim's cookies). For the entry-points group.
- **Embedded apps' `/_quillo/posted/<id>`:** one person's app. Not probed.

### Group L: live data, channels, media, sockets

#### Question 3: `data-l` written with `json_text`

Traced and run, with no finding. `data-l` (src/interp.rs:7200-7223) is the live map written plainly: `{ name: { f, a, t, h } }`, with `a` the loader's arguments. A one-`$`-key map in the arguments goes out untagged, for example `"a":[{"$quillo_err":"hello"}]`. Next to it, `data-p` tags it as `{"$":["$quillo_err","hello"]}` (seen on `GET /feed?k=hello`, `AUDIT/work/L/app6.qwe`). The browser does untag it (`islandJSON` → an `Err`), but it never reads `a`. `liveSig` (src/client/runtime.js:9792) uses only `info.t` and `info.h`. A refresh sends `args()`, which is computed from the properly tagged props. A visitor's map therefore changes nothing in the page or on the server. Channel messages and SSE data are parsed with `readJSON`, without untagging, so the same map stays a map there too.

#### Checked and sound

- Cross-site WebSocket/SSE hijacking: `/_quillo/live` (WebSocket and long poll), `/_quillo/wt` and `/_quillo/wt-ticket` refuse a foreign `Origin`, `Origin: null`, `Sec-Fetch-Site: cross-site` and `same-site` with 403. `/_quillo/events` exists only under `--watch` and has the same gate. A request with no `Origin` (not a browser) passes, by design. A page on another site cannot get a ticket, so it cannot join, follow or send as the visitor.
- Tickets: a forged session in the payload gets 403, a bad signature 403, and the ticket expires after 120 s (403). The tenant inside it is bound: an acme ticket used on evil's host still joins acme's room. The seal's purpose cannot be confused with a session's signature context. Reuse within 120 s is allowed (bearer, documented), and a connection keeps the session it connected with (documented). Tickets are not written to the access log.
- Q-642/Q-643 regression: a stream with no `watch` gets no viewer (`cam`); `publish` is enforced (only ana could send `cam`); a page's frames reach only a stream whose channel it joined; joining `media:` (empty name) is refused.
- Q-644 regression: tenants' rooms are separate. A broadcast from an acme request reached acme's page and not evil's, and evil's message reached only evil's room. An evil page watching `call:1:ana` got none of acme's frames.
- Q-666 boundary (cross-tenant topics): an evil page following `acme@news` heard nothing, whether the topic was in the address, sent down the socket later (`page_said`), in a long poll, or in a `tm=1` POST body. A bare-domain page was not told either. Mount prefixes (`/a/b|acme@x`) are stripped once and checked.
- Live cache: `mine` (reads `session()`) gave ana, bob and anon their own values. Bob sending ana's `x-quillo-had` got a whole value, not a patch. The same held for tenants: evil sending acme's hash of `news` got its own value. The browser's `livePatched` writes keys with `defineProperty` (no `__proto__` pollution).
- WebSocket framing on `/_quillo/wt` (`Reader`): an unmasked frame, a continuation with no start, invalid UTF-8, a new message before the last ended, a ping of 126 bytes, a fragmented ping, a length of 2^63, a length of 1 MiB + 65, RSV1 set (no compression extension is ever agreed), opcode 3, and close code 999 each end the connection cleanly. A fragmented text message with a ping between its parts is put back together, and the ping answered. 2,000 pings got 2,000 pongs with no growth (the queue is bounded). 100,000 levels of JSON are refused by depth.
- WebSocket framing on `/_quillo/live` (`frame_heard`): close 999, a 1-byte close and a 130-byte close are each answered 1002 and shut. An unmasked text is skipped and a later ping still answered. A binary frame claiming 2^62 bytes is read and dropped without holding memory. A `tm=1` first frame claiming 2^40 bytes is refused at once.
- SSE `events`: data is one line of JSON, and `event`, `id` and `retry` have CR, LF and NUL removed, so no event injection.
- Channel relays and service channels: signed over time, purpose (`_channel-relay`, `_channel:<name>`), target and body, so a message cannot be redirected to another channel, tenant or process. Replay within 300 s is possible (no nonce kept, known from FEEDBACK71), but needs someone who can read loopback (or the services' network) traffic.
- UDP: Quillo itself answers nothing on UDP but QUIC, which has quinn's anti-amplification. RTP ports never reply, and `udp.reply` is the program's own choice, as is a spoofed `from`. The queue has a 64 MiB byte cap.
- `/_quillo/channel/`, `/_quillo/udp/` and `/_quillo/later` need the job secret. Under a remote service's prefix, `channel-relay` and `service-channel(s)` are passed on but still need the service signature.

#### Not covered

- WebTransport/QUIC was not exercised: there is no QUIC client here and no browser is allowed. Q-1074 is PLAUSIBLE for that reason, and WebTransport datagram handling was read, not run.
- Media frames from a publisher are passed to viewers' WebCodecs unchanged. The browser decoders' attack surface is out of scope.
- `--instances` and `--register` clusters (whether broadcasts and live topics are shared between servers) were read in `cluster.rs` (signed, nonce kept) but not run.
- Memory taken by many pages following many long topics (256 × 448 bytes per page, copied about three times in `by_topic`, the group and its key, so about 1.4 GB at 4,096 pages) was worked out on paper, not measured, and is smaller than Q-1073.
- `/_quillo/media.js` and the `<quillo-media>` element were read only (a static script, and an insertion that uses the program's own base).

### Group N: new in 0.8.6/0.8.7, and the batch's fixes

#### Checked and sound

- Offline marks: a page that reads `cookie()`, `request.headers`, `__request`, `request` handed to a
  helper or a middleware's read past the session is not kept (`quillo check` warns; no
  `x-quillo-offline`); `session()` alone is kept `private` under the session's tag; a `stream()` or
  `defer` part cannot read the visitor after the headers (it gets `nobody`/`null`), so a streamed
  offline page carries nothing of theirs; `live` runs that read the visitor are never shared; a
  program's own `x-quillo-offline`/`x-quillo-session` headers are taken off.
- The gate itself in Chrome: with the session cookie and `quillo_tag` gone (normal and off-site
  sign-out), a kept private page opened offline shows the offline page and its cache is dropped;
  `start()` puts the gate first; CSP hash naming of the gate.
- Worker scope and script: fixed `<base>/_quillo/sw.js`, `Service-Worker-Allowed: <base>/`, the query
  ignored; only `/_quillo/[brsti]/<hash>` from cache and only when `immutable`; POST navigations
  never kept; kept copies keyed by the full URL (a query string poisons only its own address); a
  cross-site navigation can only make the worker drop copies, never keep another's; `Set-Cookie` is
  not replayed from the Cache API.
- Server's `DOCUMENT_NAMES` (Q-979) against Chrome's `document` prototype chain: all 309 names
  covered, none missing.
- Foreign content (Q-969/Q-990), 25 shapes rendered and read raw: `raw()` and text in `<script>` and
  `<style>` inside `<svg>`/`<math>`, breakouts (`<p>`, `<font color>`), `foreignObject`, `desc`,
  `title`, `mi`/`mglyph`, `annotation-xml` (literal and dynamic `encoding`), views, markup made apart,
  `]]>` split; HTML `<script>{text}</script>` writes `<`; `<SCRIPT>`, `<Script>`, `<scrIpt>`,
  `<STYLE>` refused or a view name. (Raw in `<xmp>`/`<iframe>`/`<noscript>` is raw by intent.)
- `local_path` (Q-968): 1,654 inputs (dot segments in every encoding, `\`, tabs, CR/LF, VT/FF, NBSP,
  U+0085, U+3000, fullwidth and division slashes, `%2f`/`%5c`/`%252f`, `;`) give the same answer on the
  server and in the browser's `local_path` (run in Node), and every non-null one resolves on the
  site with WHATWG `URL`.
- Q-966 behind `--trust-proxy 127.0.0.1`: `http` Origin against `https` refused (any case, trailing
  slash, `ws:`, `//host`, `null`); the last X-Forwarded-Proto item wins as documented.
- Q-986: without `--trust-proxy`, or with one that is not the peer, a client's `X-Forwarded-Proto`,
  `X-Quillo-From-Trusted-Proxy`, `X-Forwarded-For` naming the proxy or a forged `x-quillo-scheme`
  word change neither `delete_cookie`'s `Secure` nor the Origin gate. fcgi (Q-987) by reading:
  `HTTP_X_FORWARDED_PROTO`/`HTTP_X_QUILLO_SCHEME_FROM` dropped, names limited to `[A-Za-z0-9_]`.
- Q-972: CR/LF/NUL/U+0085/U+2028 in header names and values (`http.request`, `http.stream`,
  `http.post`, `host`) are `Bad Header`; CR/LF/space in the URL are percent-encoded or dropped by the
  URL parser; non-token methods refused for `http.request` and `http.stream`.
- Q-973/Q-974 by reading: site calls go through `follow` with the policy at each hop, same-origin
  redirects only, `Set-Cookie` never kept on a server; `cache_key` covers method, URL, every option,
  body and sorted headers; ureq is built without its cookie store.
- Web Push: endpoints held to https:443 and public addresses at bind and again at send
  (`resolve_public` at connect; `broadcasthost` refused at send); 40 address forms tried
  (IPv4-mapped, decimal, hex, short, NAT64, 6to4, IPv4-compatible, link/site/unique-local, CGN,
  `localhost.`/`*.localhost`, userinfo, other ports, non-ASCII digits) refused; no redirects
  followed; `[http]` policy asked; VAPID `aud` is the endpoint's origin, `exp` 12 h; binds need a
  session with `user` or a sealed ticket bound to session, place and person; DELETE only unbinds
  one's own; JSON content type and the cross-site gate on `/_quillo/push`; secrets kept out of logs.
- `public_ip` misses `::ffff:0:0/96` (SIIT "IPv4-translated", e.g. `[::ffff:0:7f00:1]`, accepted at
  bind and by `public_only`), but macOS answers "No route to host" for it; only a network with a SIIT
  translator would route it. Worth adding to `public_ip`, not exploitable on a stock host.
- Service callers (Q-970/971/988/989/1000) by reading `svcauth.rs` and `service.rs:1840-1960`: with
  `keys` set, a shared-secret call names no caller at all; XFCC parsing refuses unbalanced quotes,
  out-of-order or extra keys, two URIs/subjects/CNs, slash-form subjects, a CN with `:`; JWT refuses
  `none`, requires `exp`, checks `iss`/`aud` when set, no HS/RS key confusion.

#### Not covered

- An end-to-end Web Push to a real push service and Chrome's `pushManager` (outside hosts are off
  limits); sends were checked against a local sink with `QUILLO_PUSH_LOCAL=1`.
- `quillo fcgi` with a FastCGI client (read only, see above), and `--split` services' scheme word
  end to end.
- `channel.send`/`channel.broadcast` under a prefetch (named in Q-1062 from the code; not run).
- Installable apps' icon decoding with hostile images (bounded by `imaging::limits()`, not fuzzed).
- The client runtime's own rendering of spreads into SVG/MathML in the browser beyond the server's.

Main session's note: Quillo Coder's coverage tests later found two shapes the foreign-content list above did not try, Q-1112 (an unquoted attribute value ending in `/`, or a quote inside one) and Q-1113 (comments ending `--!>`, `<!-->` or `<!--->`). Both get around Q-969's fix; see the start of this report.

### Group S: the sandbox and memory safety

**Checked and sound:**
- Name resolution: 40 probes found nothing outside the safe names and pure `date`. They covered method calls and fields (`"x".print()`, `xs.print`, `value.read_file`, `(1).now`, `upper.env`), pipes (`"HOME" | env`), the names `request`, `session`, `__site_call`, `__service`, `__record`, `json`, `math`, `js`, `run_script`, `globals` and `raw`, `date.today()` / `date["now"]` / `get(date, "now")`, and env reads hidden in lambda defaults, `match` guards and templates. Every one was refused or null.
- `{"$site": ..., "$url": ...}` literals are not sites (Q-640 holds): `.notes()` says "map has no method".
- A db connection map routes calls only through `db.*` builtins, which have the db effect and are refused. An effectful builtin passed in `vars` (`env`) is refused.
- Effects: script lambdas run with no effects even when a host helper or a safe builtin (`map`, `sort_by`, `reduce`) calls them (`script_mask`). A host helper passed in keeps the host's effects, as the guide says.
- Lambdas cannot escape through the result: `holds_function` checks lists, maps and error payloads. A returned `Builtin` is a pure one.
- `type`, `fn`, `route`, `service`, `state`, `use`, `reply` and markup are all refused. `type` would need the top level in any case.
- `request` passed in `vars` is plain data (no functions).
- The JIT is off for scripts (`fuel != u64::MAX`) and under `--max-compute`.
- A panic in a request drops and re-forks that worker's interpreter (server.rs:4855), so a script's fuel or effect mask cannot carry over to the next request.
- Parser: 20 kinds of chain 200,000 deep each give "the code nests too deeply" with no crash. The kinds were `+`, `*`, `&&`, `??`, unary `!`, `.a`, `[0]`, `.len`, `|`, parentheses, lists, maps, templates, lambdas, `else if`, `? :` and string `+`. Backtracking (parentheses, `?`, lambda defaults, nested ternaries up to 250 deep) parses in under 50 ms.
- Recursion through lambdas, `map`, `reduce`, `sort_by`, templates and `match` stops at the step budget or grows the stack. Values nest at most 256 deep.
- Integer and number edge cases give errors, not panics. Tested: i64::MIN with `/ -1`, `% -1`, `abs`, `pow`, `-`; `slice`, `take`, `drop`, `set` and indexing with i64 extremes; `round` with extreme digits; `date.*` with huge offsets and years; NaN and inf.
- `str * int` and `list * int` are refused, ranges are limited before they are made, and `join` / `replace` / `split` are checked before they run.
- Fuzzing: 3,000 mutated scripts through `run_script` and 1,200 through `check_program` caused no panic.
- `check_program` type messages show types, never a used `.qwe` file's values. Paths stay inside the folder (Q-475 / Q-703 hold).
- Number parsing stays linear: `int`, `float`, `dec`, `money.of`, `money.parse` and `json.parse` of 1,000,000-digit numbers and million-digit exponents each take 2 ms or less.
- JIT against the interpreter (`QUILLO_JIT=0`) gave identical output. Tested: 2,469 results of 8 numeric functions over int, float and bool edge values (overflow, NaN, -0.0, inf, recursion); 56 results of compiled loops over mixed-type lists, nested lists, negative and out-of-range indexes, a change of type at round 300 to 500, multibyte and emoji texts, and map counts.
- `unsafe` blocks read and judged sound for their stated invariants: `append_text_as` (Arc uniqueness via `get_mut`, layout test), `push_short` / `copy_short` (room first, ≤ 64 bytes), `next_special` SIMD bounds, `jit::call` (arguments bounded to 16, cache keyed by types and arity, dependencies checked by address), `VarsCell` (lock or interpreter lock).
- FFI: scripts reach `native:` functions only if the program passes them in. `check_program` reads `.qwer` / `.rs` as text: no build, no dlopen.
- A `--max-compute` stop inside a script comes back as the script's error value (the route answered 200), unlike a `--max-memory` stop. The host is still stopped at its next check (`OVER_TIME` stays set), so this is no way around the limit.

**Not covered:**
- A line-by-line audit of all ~180 `unsafe` uses. Not done: server.rs (27), jitloop.rs (33: only tested by comparing with the interpreter), the coroutine transmutes in httpd.rs, par.rs, native.rs, fcgi.rs, platform.rs, stream.rs, debug.rs.
- Data races between `par.rs` / `parallel` threads and shared scopes: not tested (no tool to observe them without a build).
- The JIT's caches are keyed by the `FnDef`'s address (`jit.done`, `LAST`). An address reused after a reload could find another function's code. Only `--watch` or `--reload` (Q-663's area) frees `FnDef`s, so this was not pursued.
- foreign.rs was not read. Deep JSON from visitors relies on Q-875's cap of 128 (read in value.rs, not re-run). Form and query bracket nesting was left to the HTTP auditors.

### Group D: data, crypto, sessions, tokens

#### Checked and sound

- Session cookie at the root: `__Host-quillo_session; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=Lax`; `Cache-Control: private` and `Vary: Cookie` on the answer.
- Session tampering: payload changed (`bob` to `adm`), signature changed or lengthened, `=` inserted, the cookie under `quillo_session` or `__host-...`: all `{}`. Junk second cookie beside a valid one: still the valid session.
- Expiry: `max_age: 1` is no session after 2 s. A root cookie is no session under `--base /app` (the signature binds the mount and tenant).
- Signing key: `QUILLO_SECRET` through HMAC-SHA256 to a 32-byte key; without it a random 32-byte key from the OS (`getrandom`), with a warning; a warning for secrets under 32 bytes; no fixed or default key in source, guide or `quillo new`. Domain separation between session, `seal`/`digest` purposes, offline tag and VAPID (HKDF) holds: contexts start with `/` or NUL, purposes with `quillo`.
- Session id: 128 bits from the OS; rotated from one `user` to another; not a credential. Sign-out leaves old copies valid until expiry, as the guide says.
- Constant-time compares: `crypto::same` for sessions, seals, service signatures, the job token, `crypto.equal`; the health token compared in constant time.
- Passwords: Argon2id m=19456 t=2 p=1, 16-byte random salt; a missing or junk hash is checked against a stand-in (same time) and is false; at most one hash per core, 1,024 waiting.
- `crypto.token`: OS randomness, 1 to 1024 bytes. No encrypt/decrypt builtin exists (Web Push's aes128gcm is per message with a fresh salt and key).
- JWT: `alg: none`/`NONE` refused; HS256 signed with the RSA public key refused when only `jwks` is set and when a secret is set too; 1024-bit RSA key refused (ring needs 2048); DER ES256 signature refused; key of another `kty` for the `kid` refused; embedded `jwk`/`jku` headers ignored; expired (past 60 s leeway), no `exp`, `exp` as string or float, future `nbf`, wrong or missing `iss`, wrong `aud` all refused; `aud` arrays read; valid RS256, ES256 and EdDSA accepted. 15 tokens with unknown `kid` caused 0 extra JWKS fetches (at most one a minute); the JWKS agent follows no redirects.
- Q-581 direct forms: `set_config(..., false)` outside a transaction refused; `db.transaction` and `db.exec_many` connections reset (8 later requests saw no setting).
- Tenant names (`tenant_name_ok`) restrict the `create schema "..."` and `create database` names; `sql_name` admits only letters, digits, marks and `_` (63 bytes); no value is ever spliced into SQL text by Quillo (all bound).
- `.env`: no variable expansion (`${D}` literal), `\n` inside a value never starts a variable, an unclosed quote skips only its own line; real environment wins over the file.
- `locale_from`/`Accept-Language`/`locale` cookie: every result passes through `I18n::best` to a known locale, so no arbitrary locale text reaches files or keys; a failing rule is ignored.
- `src/privacy.rs` is name scoping (`private fn`), not a data boundary; `private server fn` is an error.

#### Not covered

- MySQL/MariaDB session state across pooled connections (Q-581's MySQL side): read only; relies on mysql_async 0.37's default `reset_connection`. Not run (MariaDB is installed but time went to PostgreSQL).
- OAuth client credentials and client-side mTLS: read only (form/basic encoding, no redirects, token cache keyed without the secret); no token endpoint run against a caller.
- Idempotency-Key owners and the session's hidden id kept from no user to a user: read only; abusing it needs a planted cookie (Q-1077) and the victim's key.
- MCP tokens and push subscriptions (`digest`, `seal`): read for domain separation only; their flows are other groups' areas.
- PostgreSQL `standard_conforming_strings=off` / MySQL `NO_BACKSLASH_ESCAPES`, where Quillo's tokenizer could number placeholders differently from the server: not tried.

### Group F: files, uploads, rendering helpers, outbound HTTP

#### Checked and sound

- `files("public", path)`: `.env`, `%2eenv`, `.ENV`, `.env.`, `.env%20`, `.git/config`, `%2egit`, `sub/../.env`, `sub%2f..%2f.env`, `%2e%2e%2f`, `%252e` (double encoding), `..%5c`, overlong `%c0%ae`, `%00`, full-width dot, `//etc/hosts`, `%2fetc%2fhosts`: all 404. Links to `../.env`, `../main.qwe`, `/etc/hosts`, `../../` (up2/main.qwe): 404. Backslash in a name serves that file only.
- `file()` with `:name` / `*path` from the request: `..%2f`, `%2e%2e`, `..` refused (404); serving `.env` inside the given folder is documented and warned by `quillo check` (Q-657).
- `quillo serve`: `.env`, `.env.production`, `.git/config`, `lib.qwe`, an unopened SQLite file, `node_modules/.pnpm/x.js` all 404.
- Ranges: `0-0`, `-5`, past-the-end start (416), `5-2` (416), `-0` (416), huge end (clamped), several ranges (whole file, 200), `0x1-2` (200). `If-None-Match` exact and `*` give 304. Large-file HEAD gives the right length with `nosniff`.
- Every answer carries `x-content-type-options: nosniff`; unknown extensions are `application/octet-stream`.
- `file(..., { download })`: a name with CR/LF/quote becomes `_` in `filename=` and percent-encoded in `filename*`; no header injection.
- Multipart: 10 MB bodies built to stress `find` (boundary of 70 `-` over `-`x71 runs; `\r\n--B..x` repeated): 50 ms worst, linear. Upload body isolation: a concurrent request with files does not see another's, a forged `{start, end}` record reads only its own request's body.
- Guide upload examples: saved under `uuid()` names with a fixed extension and served by `file()` with the extension's type and `nosniff`; no same-origin HTML/SVG from them.
- Markdown (server builtin and `/_quillo/markdown`): `javascript:` in every tried form (case, `&#x09;`, `&#106;`, `&colon;`, `&#58;`, autolink, `<...>`, reference, image, leading space, `&#10;` inside `<>`), `vbscript:`, raw HTML and comments (escaped), title attribute quotes, fence info string (filtered to `[A-Za-z0-9-]`), code bodies (escaped by highlight), tables, `markdown_page` title. `/_quillo/markdown` needs JSON and same-site and answers JSON.
- Highlighting: 22 pathological 0.5-0.9 MB inputs over js/html/css/python/rust/sh/qwe/json/sql: all under 0.06 s, output at most 15x input.
- Outbound header injection: CR, LF, CR LF, VT, NEL in a header value, and CR LF, space, colon, empty in a header name: all refused before sending. CR LF in a URL path is dropped by the URL parser; in the host, refused.
- `public_only` and `private = false` refuse 127.0.0.1, `0x7f.1`, `2130706433`, `0177.0.0.1`, `127.1`, `0x7f000001`, `017700000001`, `0`, `0.0.0.0`, `[::ffff:127.0.0.1]`, `[::1]`, `[::]`, `[::127.0.0.1]`, `[64:ff9b::127.0.0.1]`, `[2002:7f00:1::]`, `localhost`, `LOCALHOST`, `localhost.`, `a.localhost`, `127.0.0.1.`, `%31%32%37.0.0.1`, ideographic dots, full-width digits; zone ids do not parse. The resolver that judges a name is the one ureq connects with (no rebinding without a proxy).
- Redirects: each hop judged by `[http] hosts` (localhost, `[::1]`, `//[::1]`, `http:/\\localhost`, `127.0.0.1@localhost` refused); `file:` and `gopher:` locations are not followed; `authorization`, `x-api-key` and `cookie` are dropped on a redirect to another port or host, kept on the same origin.
- Decompression: gzip and deflate bombs are cut at 64 MiB with an error (see Q-1072 for the memory); `http.stream` reads a 200 MiB gzip answer in pieces (RSS back to 14 MB).
- No `HTTP_PROXY` from FastCGI parameters or request headers (no httpoxy): only `.env` and Quillo's own variables are set in the environment.
- JPEG progressive scan DoS: zune-jpeg caps scans at 100. PNG `zTXt`/`iTXt` are kept compressed and counted in the budget.
- `entities.rs` decodes only literal markup; `styles.rs` evaluates component CSS at load time from globals, with no request data.

#### Not covered

- WebP lossless and GIF decoder internals were read only at the level of the image crate's limits; no crafted files were run.
- DNS rebinding through an `HTTP_PROXY`, and `public_only` checks against names, could not be run without an outside DNS resolver (documented behaviour: the proxy looks the name up).
- Web Push endpoints (`src/push.rs`) and `udp.listen`'s default address were only glanced at; push has its own endpoint check and is left to the PWA audit.
- TLS options (`ca`, `cert`, `key`) and the `cache` key (Q-974) were not re-tested.
- The browser runtime's `t_html` (Q-1059) was traced in runtime.js, not run in a browser (no browser in this brief).

### Group G: memory and CPU under a cap

#### Checked and sound

- Hash flooding of request data: query, form, cookie and header maps, `request.json` objects and
  serde's maps are `IndexMap`/`HashMap` with std `RandomState` (SipHash, keys per process).
  `unique`'s table uses a process-keyed mix. FxHash (`BuildFx`) is used only for program names
  (`Vars::Many`, the symbol table) and the program's SQL text (src/sql.rs:1839). The only weak
  hash on visitor data is Q-1092.
- Symbol interner: request data never becomes a `Sym` (every `Sym::new` outside the parser takes a
  program name or a constant) except Q-1076. A script's own identifiers become `$1..$k` and are
  reused, so they are bounded by the most names in one script.
- JSON nesting: 500,000 levels deep (lists, and maps at 300,000) to `/json`, `/_quillo/rpc/tally`,
  `/_quillo/markdown` and a typed route gave null, or 400 "nests more than 128 deep", in about
  1 ms. No crash, server alive.
- Huge numbers in JSON (a 1,000,000-digit integer, `0.000...1e999999`, `1000...e-1000000`, 200,000
  × `1e308`, 1,000,000 decimals): 2-8 ms each. `dec` text is capped at 400 characters and a
  6-digit exponent (src/decimal.rs:338).
- Head limits: more than 64 KiB is 431/414; a 101st header is 431. 7,000 `tag=` values to a
  `[str]` query took 2 ms, 10,000 extra parameters 1.6 ms, 98 headers of 600 bytes 0.3 ms. Cookies
  are bounded by the head.
- Multipart: a 1,001st part is 413 before the route. A 10 MB body made of near-matches of a
  70-dash boundary took 51 ms; 999 parts took 5 ms.
- WebSocket reassembly: two parts totalling 1.1 MB, a frame claiming 2^63 bytes, and a continuation
  claiming 2^64-1 are each closed with a Close frame. The claimed length is checked before
  anything is allocated (Q-593 holds).
- Chunked bodies: a size line is at most 1,024 bytes, extensions and trailers at most 8 KiB in
  all, and each size is checked against `--max-body` before its data.
- Markdown door, other shapes, 300 KB each (emphasis, links, tables, reference definitions,
  entities, HTML, fences, strikethrough, backticks, 1,000-deep lists): at most 44 ms, small memory.
- Slowloris against `sweep` (one connection each): a head sent 1 byte every 10 s got 408 at 30.7 s;
  a connection that sent nothing was closed at 60.8 s; a body at 600 B/s got 408 at 30.7 s.
  `--drain 5s` ended a held stop 5.2 s after SIGTERM.
- The head is parsed again on each read: trickling a body in 1-byte segments behind a 58 KB head
  cost about 36 µs a segment (100,000 segments/s, 72% of a core). GET / on the same instance stayed
  at 0-12 ms. That is about what a small request costs per packet.
- `--max-memory` on its held paths ended the request with a clean 500, and the process went on: a
  JSON body under 8MB, and markdown's written answer past 8MB.
- Bounded caches keyed by visitors: live values (64 MB) and patches (256), the push rate limit
  (pruned past 10,000), "said once" sets (64), live topics (256 a page), channel inboxes (4,096)
  and names a page asks of (256), regex cache (256 patterns / 32 MB a thread, Q-975).
- A server fn argument of the wrong type gets a 400 without the value; a 1 MB string did not reach
  the log (37 bytes).

#### Not covered

- Clients from other addresses: loopback is exempt from `--connections-per-client` (512 per address
  or IPv6 /64), and RULES allow only 127.0.0.1. From the code: nothing bounds the total of bodies
  being received. Each connection may buffer up to 64 KiB + `--max-body` + 64 KiB (`input_cap`),
  so one address can hold about 5 GB by sending at 1 KiB/s per connection for long enough. Memory
  equals bytes sent, but no global cap exists (nginx spills to disk).
- 10 MB versions of Q-1052, Q-1057, Q-1076 and Q-1075, and an actual OOM kill: extrapolated, because the probe
  caps did not hold the process.
- `/_quillo/mcp` (its JSON is read after the caller check, by the same `json_value` as Q-1052, so a
  signed-in user or token holder could reach Q-1052's cost there), signed service calls, FastCGI,
  WebTransport (QUIC), UDP and media streams: not probed.
- `--instances N` concurrency (each instance runs Q-1052/Q-1069/Q-1070 one at a time, so N at once) and
  `--split`: reasoned, not measured.
- Drop recursion (Q-661, known) and PostgreSQL rows (documented as not held).

## Not audited: group T, developer tools and the supply chain

The auditor for group T was stopped by the model provider's safety checks part of the way in ("Opus 5.5's safeguards flagged this session", `[cyber]`). It wrote no findings and left no server running. As FEEDBACK71's notes ask, it was not reworded and run again in the same session. Redo it in another session, or with Defense Access in the Cyber Verification Program (https://support.claude.com/en/articles/14604842-real-time-cyber-safeguards-on-claude). What it was to cover:

- The development server: what `quillo run --watch` and `/_quillo/dev` expose (source, error text, the restart), whether another site's page or DNS rebinding (`Host: evil.test` to 127.0.0.1) reaches it, and its default address.
- MCP over HTTP (`/_quillo/mcp`): who may call it, Origin and Host checks against DNS rebinding (the MCP specification requires them), what tools and resources reach whom.
- `quillo dap`: whether it listens on a socket, where, and with what authentication.
- `quillo install` and native plugins: registry redirects, integrity, and a cloned repository that ships `.quillo/native/*.dylib` with a matching `plugins.lock` (does `quillo run` load it instead of building the `.qwer` the developer read?). Q-981 and Q-983 are known.
- `quillo serve`'s default address (its serving of `quillo.policy` is Q-1088), `quillo fcgi`'s Unix socket permissions, `--register` and `/_quillo/announce`, `src/cluster.rs`.
- The desktop and phone bridge (`SHELL_JS`, `desktop.rs`'s ipc handler takes the Origin from the message's URL and does not check the frame), and the four bugs an earlier review of `embed.rs` reportedly found, with their fixes deferred.
- The editors in `~/dev/quillo-editors` after Q-967's fix: workspace settings (`quillo.path` in `restrictedConfigurations`?), tasks, code actions, other editors' plugins.

Also not done by any group, and worth a later pass:
- A line-by-line audit of most of the roughly 180 `unsafe` blocks, and thread data races (group S fuzzed and compared compiled against interpreted code instead).
- The WebP and GIF decoders.
- MySQL's pooled session state.
- A live OAuth or mTLS caller.
- WebTransport over a real QUIC client (Q-1074 is PLAUSIBLE for that reason).
- Clients from non-loopback addresses (`--connections-per-client`, `--max-connections`).
- Firefox and Safari.

## Where the AI struggled

The main session's own slips while checking findings again, in the order they happened. The auditors' slips are in their findings files.

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `quillo run info.qwe icc1k.png` | the habit of `python script.py arg` | `unknown argument icc1k.png: a program's own arguments go after --`: clear, and it said exactly what to write | 1 run |
| `args[0]` | `sys.argv[0]`, `process.argv` | `cannot index fn with int`: right, but it does not say `args()` is a function to call | 1 run |
| `let t0 = date.now()` ... `date.now() - t0` | `Date.now()` in JavaScript is a number | `cannot subtract str and str`. `date.now()` is a date text, and the guide's millisecond clock was not where I looked. I timed the process with `/usr/bin/time` instead | 2 runs |
| `curl ... 127.0.0.1:4920/login?user=x` in a zsh loop | bash habit | zsh's `no matches found` for the `?`, once per try: the readiness loop waited its full 10 s. Not Quillo's | 10 s |
