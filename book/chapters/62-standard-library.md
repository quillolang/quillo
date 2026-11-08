# Standard library

- **General:** `print str int float dec len type_of`
- **Strings:** `upper lower trim split join contains starts_with ends_with replace slice repeat index_of normalize`
  (`upper` and `lower` know all of Unicode: `"straße"` is `"STRASSE"`; `trim` takes off
  Unicode's White_Space (U+0085 too, not U+FEFF); `replace` replaces every occurrence of
  plain text, not a pattern, and an empty pattern puts the text before each character and
  at the end (`replace("abc", "", "-")` is `-a-b-c-`); text orders by code point; each the
  same in the browser; regular expressions are `re`,
  below). `normalize(s, form)` *(since 0.7)* gives one Unicode form of the text, `"NFC"` unless
  `"NFD"`, `"NFKC"` or `"NFKD"` is given, on both sides: an `é` typed as one character and
  one typed as `e` and an accent are two texts until normalized. Compare emails and user
  names normalized, and user names in `"NFKC"`, which also makes `ﬁ` `fi` and a
  full-width `Ａ` an `A`: `lower(normalize(trim(name), "NFKC"))`.
- **Regular expressions:** `re.test(text, pattern)` (true or false), `re.find` (the first
  match, or null), `re.find_all` (every match), `re.replace(text, pattern, with)` (every
  match; `with` is text where `$1`, `$<name>`, `$0` and `$$` stand for the groups, the
  match and `$`, or a function given the match) and `re.split`. A match is
  `{ text, start, end, groups, named }`, positions counting characters as `slice` does.
  Flags come last: `"i"` (any case), `"m"` (`^` and `$` at each line, a line ending at `\n` or `\r`, `\r\n` as one end, on both sides) and `"s"` (`.`
  matches line breaks too). `re.valid(p, flags)` *(since 0.7)* asks whether a pattern can be run, for
  one from data. They give the same answers on the server and in the browser, but for
  groups in a loop (`(?:(a)|b)*` on `ab`, where the server keeps the group's last match and
  the browser clears `(a)` on each round: the two engines cannot be made the same, and
  `quillo check` warns of a capturing group inside a repeated one; capture the whole
  repetition, `((?:a|b)*)`), and not at the same cost: the server's engine takes the same time on any text, and the
  browser's backtracks, so `(a+)+$` can hang a tab with no way to stop it, which `quillo
  check` warns of. Bound the counts (`{1,8}`), or run a visitor's own pattern in a `server
  fn`. So a pattern takes only what both read alike: characters, `.`, `[...]`,
  `\d \w \s \b` (ASCII digits and word characters, as in JavaScript) and their capitals,
  `^ $`, `(...)`, `(?:...)`, `(?<name>...)`, `|`, and `* + ? {n} {n,} {n,m}` (lazy with
  `?`). Lookaround, backreferences and `\p{...}` are errors, found by `quillo check`
  when the pattern is written in the call, and so is escaping a character that needs
  none (`\\,`), a flag given twice, and a count over 1,000 (`{1001}`). In a string, a
  backslash is written twice: `"\\d+"`.
- **Translations:** `t(key, vars)`, `locale()`, `translations`, `locale_from` and
  `with_locale` (see [Translations](#translations)).
- **Markdown:** `markdown(text)` gives HTML (CommonMark with tables, strikethrough and task
  lists). HTML inside the text is shown as text and `javascript:` links are blocked, so
  it is safe for what visitors write; `markdown(text, { html: true })` keeps the HTML,
  for text you wrote, and `markdown(text, { inline: true })` keeps only what is inside a
  line (bold, italic, code, links) for a headline or a caption: no paragraph around it, and
  no headings or lists (their lines, broken by `<br>`). It gives an `html` value, shown
  as it is in markup; `str(markdown(text))` is its text, to store. In a component the
  server makes it (a request to `/_quillo/markdown`, so the page shows the same HTML,
  highlighting included), awaited like a `server fn`: call it in an event handler or a
  `state` initializer, and keep the text in state:
  `oninput={(e) => { preview = str(markdown(e.value)) }}` and `<div>{raw(preview)}</div>`.
  `/_quillo/markdown` takes at most 1 MB of text (a larger one is answered 413, and
  `markdown` in the component gives an error value), and one that computes past
  [`--max-compute`](#options-of-quillo-run) is a 500.
  `markdown_page(text, { title, lang, html })` is the same as a whole page, for a route:
  a doctype, a `<head>` with its `<title>` (the option's, else the text's first level-one
  heading, `# Title` or a line underlined with `=`, as the text it shows, without its
  marks; a `#` line in a code block or in HTML is no heading), and the HTML in its `<body>`. Markdown takes time in proportion to the text,
  whatever marks it holds (a megabyte of `*a_` is 0.03 s), and `markdown`, `markdown_page`
  and `highlight` end at `--max-compute` as a loop does. A fence's language, for
  highlighting, is its first word, to a space or a comma (```` ```rust,ignore ```` is Rust).
- **Lists:** `map filter find any all reduce sum min max sort sort_by reverse first last push range enumerate slice unique flatten zip take drop count group_by join_on index_of`
- **Maps:** `keys values get has set remove merge` (`get(m, k, default)`; `merge` takes
  two maps; there is no `entries`: `for [k, v] in m`). A map keeps the order its keys
  were set, number-like keys too, in the browser as on the server.
- **Math:** `abs round floor ceil sqrt pow`, and the three modules under
  [Numbers and data](#numbers-and-data). For money, [Decimals](#decimals) and
  [Money](#standard-library-money); `format_number(x, locale, digits)` writes a number as a locale does.
- **Records:** `type User = { ... }` makes `User(fields)`, which builds and checks one
  (see [Records](#records)).
- **Errors:** `err is_err unwrap`
- **Web:** `raw escape url_encode local_path env now file files markdown_page redirect respond json.parse json.stringify
  cookie set_cookie delete_cookie session set_session end_session site_headers`
  (`local_path` under [Route groups](#routes-route-groups), `site_headers` under [What a route
  returns](#what-a-route-returns)). `url_encode("a b")` is
  `a%20b`, and `url_encode({ q, page: 2 })` is a query string (`q=...&page=2`, leaving
  out keys whose value is null); in the browser too. `env(name, default)` gives the
  default when the variable is not set.
- **Program and pages:** `highlight(code, lang)` (Quillo, JavaScript and TypeScript,
  Python, Rust, shell, SQL, CSS, HTML and XML, JSON; anything else as escaped text, no
  word coloured), the same on the server and in the browser, where `str(highlight(...))`
  is the markup too. It reads each line once, so it takes time in proportion to the text,
  and colours strings as each language reads them: Python's triple-quoted and Rust's
  strings run over lines, Rust's raw strings `r#"..."#` have no escapes, a Rust
  character such as `'"'` is a string and a lifetime `'a` is not, a JavaScript regex
  literal is one token, a shell's `it's` or a `#` inside a word starts nothing, and a
  range's `..` ends the number before it (`0..10` is two numbers); a word is Unicode
  letters and digits on both sides.
  The browser knows only its own languages: `"constructor"` and `"__proto__"` are escaped
  text,
  `check_program`, `run_script`, `delete_file`, `save_upload`, `read_upload`,
  `schedule`, `idempotent`, `parallel`, `ready`, `style_with`, `classes`, `accept_calls`, `allow_cross_site`,
  `tenant`, `tenant_from`, `with_tenant`, `platform` (below), and in components `url`, `navigate`, `every`,
  `ask`, `notify`, `js`, `js_new`, `live`, `waiting`, `publish`, `watch`, `unwatched`: each is
  described where it is used.
- **Where the program runs:** `platform()` *(since 0.7)* gives a map: `os` (`"ios"`, `"android"`,
  `"macos"`, `"linux"`, `"windows"`), `os_name` and `os_version` (`"iOS"`, `"27.0"`), `kernel`,
  `device` (`"phone"`, `"tablet"`, `"desktop"` in `quillo app`, `"server"`), `model` (the
  hardware's identifier: `"iPhone19,2"`, `"Mac17,2"`), `arch`, `cpus`, `simulator`, `runtime`
  (`"mobile"`, `"app"` or `"server"`), `locale` (the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is set and not empty; null for `C` and `POSIX`), `time_zone`, `quillo` (this build's version), and in a
  phone app also `model_family` (`"iPhone"`), `memory` (bytes), `screen` (`{ width, height, scale }`, in
  points) and `app` (`{ id, name, version, build }`). What a system does not tell is `null`. It is read
  once: the screen and locale are as they were when the program started. It runs on the server, as
  `env` does: in a component, call it from a `server fn`. `server` is where the server listens,
  `{ host, port, url, base }`: the port it got when started with [`--port 0`](#options-of-quillo-run)
  (`quillo run main.qwe --port 0`), and a `url` reachable from this
  machine (`0.0.0.0` is written `127.0.0.1`), so a program that reads its own pages need not be
  told its address again. It is null in a script and while the program loads.

A builtin given more arguments than it takes stops and says what it takes: `` `split`
takes 2 arguments, got 3: split(s, sep) `` (and for `sort` given a comparator, to use
`sort_by`), in `quillo check` where the call is written out, as the program runs however
it is called (a method on an untyped value, `line.split(",", 2)`; a builtin held in a
variable or passed to another, `reduce(xs, 0, max)`), and in the browser. Nothing is
ignored: `uuid(1)`, `random(1)` and `merge(a, b, c)` are errors.

What each does at its edges, where the answer is not what another language would give:

| Call | Gives |
|---|---|
| `sort(xs)`, `sort_by(xs, f)` | ascending and stable; `f` may give a list to sort by several keys, `sort_by(rows, (r) => [r.a, r.b])`, as lists compare item by item, and so do `<`, `>`, `<=` and `>=` (`[1, 2] < [1, 3]`); no comparator and no descending flag (`reverse(sort(xs))`, or `vec.argsort(xs, true)`). Numbers (ints, floats and decimals together), strings (by code point: `"B"` before `"a"`) and lists of those order; `null`, booleans, maps, a string with a number, a decimal with a float, or NaN are an error, in the browser too (`vec.argsort` puts NaN last), and comparing `null` says that a miss of `index_of`, `find` or `get` is `null`, not -1 |
| `split(s, sep = " ")` | at each single `sep` (`"a  b"` is `["a", "", "b"]`); `split(s, "")` is the characters; no limit |
| `join(xs, sep = "")` | the items as text, with `sep` between |
| `filled(x)` | `x`, or `null` for `""`, so `filled(name) ?? "anonymous"` replaces a blank or missing name; `0`, `false`, `[]` and `" "` stay as they are (`filled(trim(x))` counts spaces as blank) |
| `slice(x, start, end)` | negative positions from the end; both kept within the list or string; `end` before `start` gives an empty one |
| `take(x, n)`, `drop(x, n)` | a negative `n` counts as 0, unlike `slice`: `take(xs, -2)` is empty, `drop(xs, -1)` all of it, so a count worked out by subtraction stays safe |
| `range(n)`, `range(a, b)`, `a..b`, `range(a, b, step)` | `b` left out, step 1, empty when `b <= a`; a negative step counts down; a list of at most ten million, while a `for` whose list no one keeps counts without one |
| `group_by(xs, f)` | a map from each key, as text, to the items with it, in order: `1` and `"1"` are one key; an error `f` gives is given back, not made a key |
| `unique(xs)` | the first of equal items, keeping exactly what `==` keeps (`unique([1, 1.0, 1m, "1"])` is `[1, "1"]`), in time linear in the list on both sides: 40,000 items take milliseconds, not seconds |
| `zip(a, b)` | pairs, as long as the shorter |
| `contains(x, y)` | in a string, `y` as text; in a list, an equal item; in a map, a key |
| `remove(list, x)` | every item equal to `x` gone; `remove(map, k)` the key |
| `set(xs, i, v)` | `i` past the end is an error |
| `round(x)` | a float rounds half away from zero (`round(-2.5)` is `-3`) to an int, up to 2^63 (`type_of(round(1e16))` is `int`; past it, and NaN and the infinities, the float), and an int is itself (exact past 2^53); `round(x, 2)` a float with at most 15 places, from its shortest decimal (`round(1.005, 2)` is `1.01`, as `format_number` writes it); fewer than no places round to tens, hundreds, ...: `round(1234.5678, -2)` is `1200.0`, from the float's shortest digits and exact at every place (`round(1e30, -29)` is `1e30`), and a result past the largest float is an error; an int to 0 places or more is itself (`round(5, 2)` is `5`, as in Python), and to fewer the nearest multiple, exactly, a half away from zero (`round(1250, -2)` is `1300`, where Python's is `1200`, half to even); `round(a / b)` of two ints is exact (see `/` under [Values](#values)); a decimal takes a mode (see [Decimals](#decimals)) |
| `int(x)` | a float or decimal cut toward zero (an error value when too large for an int); `true` is 1; text of digits only (`int("1.0")`, `int("1e3")` are error values) |
| `float(x)` | a number as a float; text as Rust reads a float (`"2.5"`, `" 7 "`, `"1e3"`, `".5"`, `"inf"`, `"nan"`), so `"0x10"`, `"0b11"`, `"1_000"` and `""` are error values, in the browser too |
| `floor(x)`, `ceil(x)` | a float to an int, up to 2^63 (`type_of(floor(1e16))` is `int`; past it the float); an int as it is, past 2^53 too, as `round(x)` gives it; `floor(a / b)` of two ints divides in ints, exactly (see `/` under [Values](#values)) |
| `str(x)` | a whole float as `2.0` (in a component's text, `2`, on both sides), never in exponent form (`1e16` is `10000000000000000`); a float's shortest digits, a tie taking the even digit as JavaScript does (`1916218016406818.3`, which is held as ...818.25, is written `1916218016406818.2`); markup as its HTML, in the browser too; a list or map as Quillo writes it (`{ a: 1, b: "x" }`, a key of letters, digits and `_` of any script, not starting with a digit, bare (`{ é: 1, 日本: 2 }`), another quoted (`{ "a b": 3, "1a": 4 }`), on both sides); `null` as `null` |
| `type_of(x)` | one of `null bool int float dec str list map fn html response error` (`promise` for a JavaScript Promise in the browser) |
| `len(x)` | a string's characters, a list's items, a map's keys; an `html` value's bytes |
| `min([])`, `max([])`, `first([])` | `null`; `sum([])` is `0`, and a sum of decimals stays exact; `sum` adds numbers only, and text stops it at its item (`` `sum` adds numbers; got the str "12.50" at item 1: use `float(...)` or `dec(...)` first ``), where it was joined after a `0` |
| `sqrt(-1)` | `NaN`, while `math.sqrt(-1)` is an error, as in Python |
| `pow(2, 10)` | an int, exact, or an overflow as `*` gives (`pow(2, 63)`, and `pow(2, 4294967296)`: a power of 2^32 or more is exact for a base of -1, 0 or 1, and an overflow for any other); a float on either side, or a negative power, gives a float; a decimal to an int's power, a decimal (see [Decimals](#decimals)) |
| `print(a, b)` | the values as text, a space between, on stdout (the console in a page) |
| `now()` | milliseconds since 1970, an int |
| `sleep(ms)` | no maximum; other requests run meanwhile, and a request's sleep of 5 ms or more holds no thread (a timer wakes it), so a hundred streams sleeping in their loops hold up nothing; in a scheduled job or a tick, ends it (status 503) when the server stops or the program is loaded again, so a loop that runs for ever stops; a request waits it out (it is answered at a stop, up to 10 s, and finishes on its version at a reload) |
| `random_int(lo, hi)` | both ends included |
| `uuid()` | a version 4 UUID |
| `read_file(path)` | text: bytes that are not UTF-8 are replaced; a missing file is an error value |
| `write_file(path, v)` | `v` as text, folders made, the file replaced; `null` or an error value |
| `escape(s)` | `& < > " '` as entities |
| `json.parse(s)` | an error value for bad JSON, for a number past a float (`1e400`) and for half of a character (a lone `\ud83d`), on both sides; an int past 64 bits becomes a float; a key given twice keeps the last value; keys in the order written, on both sides; at most 128 levels deep, on both sides, past which it is `invalid JSON: it nests more than 128 deep` (`request.json` and an `http` answer's `json` are `null`, a stream's line is its text, and a database's JSON cell is that error); every other reader of JSON (sessions, channels, a `server fn`'s arguments, a service's body) takes the same depth |
| `json.stringify(v, pretty)` | indented by two spaces when `pretty` is true; a whole float as `2.0` (`-0.0` too), so it reads back as a float, and an exponent where JavaScript writes one (`1e+21`, `1e-7`), on both sides; markup as its HTML; a decimal as a string, NaN and infinities as `null`, an error value as `{"error": ...}`; a function is an error value naming where it is (`` json.stringify: `a.f` is a function, which cannot be written as JSON ``), where it was written as text; JSON past 256 MiB, or nested more than 10,000 deep, is an error value, counted before it is made, as lists that hold one part twice can double at each step *(since 0.7)*; a route's answer the same, a 500 with the reason in the log |

What is not in the standard library, and what to do instead:

- **Bytes, base64, hex.** There is no bytes value: files, request bodies and HTTP
  answers are read as text (bytes that are not UTF-8 replaced), and only `file()`,
  `save_upload` and `image.convert` move bytes as they are. Nothing decodes base64 or
  hex (`crypto.token` and the hashes only make them). Binary work goes in Rust (see
  [Rust](#rust)).
- **Formatting text.** No `sprintf` or padding: template strings, `repeat`, `slice`,
  `format_number`, `money.format` and `date.format`.
- **CSV, XML, YAML.** No parsers built in: `split` and `re` for simple CSV, or the
  catalog's plugins `csv`, `xml`, `yaml`, `toml`, `gzip`, `tar` and `bignum` (`quillo
  install csv`, see [Plugins](#plugins-from-the-catalog)). JSON, Markdown and regular expressions are
  built in.
- **`url_decode`.** Not there: query strings, forms and cookies arrive decoded.
- **A seed for `random`.** None: runs are not repeatable.
- **Other processes.** A program cannot run a command: `env`, files, and a command started
  beside it with the option [`--with`](#options-of-quillo-run) (`quillo run main.qwe --with "npx tailwindcss ..."`). A script reads its arguments with `args()` and its input with `stdin()` and
  `read_line()` (see [Scripts as tools](#scripts-as-tools)).
- **Hashing a value.** `crypto.sha256(json.stringify(v))`. Values never change, so
  nothing needs copying.
- **Building a long string.** `+` makes a new string each time: push the pieces to a
  list and `join` it.
- **Streaming:** `stream(f)` and `events(f)` answer as they go, `http.stream` reads
  another server's answer as it arrives, and `listen` and `receive` read streams in a
  page (see [Streaming](#streaming)).
- **Database:** `db.open db.exec db.query db.one db.transaction db.migrate db.connect
  db.tenants db.tenant_exists db.shards db.atomic db.system` (see [Database](#standard-library-database)).
- **Crypto:** `crypto.sha256(text)` and `crypto.hmac(key, text)` (HMAC-SHA256, hex),
  `crypto.token(bytes = 32)` (1 to 1,024 random bytes, base64url: 43 characters by
  default), `crypto.hash_password(pw)` (Argon2id, salted, so it needs the `random`
  effect) and
  `crypto.verify_password(pw, hash)` (false for a wrong password and for anything that
  is not such a hash), `crypto.equal(a, b)` (in constant time, for comparing secrets).
  Password hashing takes tens of milliseconds and 19 MiB on purpose, runs off the event
  loop, and at most one per core at a time (others wait their turn), so a flood of
  logins slows logins down rather than taking the server's memory.
- **Effects:** `sleep(ms)`, `random()`, `random_int(lo, hi)`, `uuid()`, `read_file(path)`,
  `write_file(path, text)` (paths stay inside the program's folder, links followed: see
  [Paths and links](#paths-and-links)). `uuid()` takes its
  bytes from the operating system, so it can serve as a session id or token; `random()`
  and `random_int()` are fast but predictable, and must not.

Safety built in:

- Text in HTML and attribute values is escaped. Keys of a `{...map}` spread on an element
  become attributes only if they are plain names that run nothing: `onclick` (in any
  case, and as `bind:onclick`), `srcdoc`, names with `:` (but `xlink:href`), `@` or
  quotes or `>` are dropped, and so are the attributes Quillo's runtime reads (`data-qi`,
  `data-c`, `data-p`, `data-s` and the rest), so a spread cannot make a component; a key
  cannot replace an attribute written out, in another case either. Event handler attributes and `srcdoc` take only text written in
  the program (see [HTML literals](#html-literals)). `javascript:` URLs in `href`, `src`
  and the like render as `about:blank#blocked`, whatever the value's type: the text the
  attribute would say is checked, so `href={markdown(u, { inline: true })}` too. So do
  the values an SVG `<set>` or `<animate>` gives another attribute (`to`, `from`, `by`,
  each of `values`), which may be an `href`.
- CSS takes data as it is written, but for what would end it: `style={...}` is escaped
  like any attribute, a style map's values and css`...`'s `${...}` have `{ } ; \` written
  as CSS's escapes, and css`...`'s `&` as `\26 ` (so a value adds no rule or declaration,
  in `<svg>` and `<math>` too), and `<style>{x}</style>` has
  its `<` rewritten (`\3c `), so none can end the element; within a value, what it says is
  CSS. CSS runs no script in today's browsers; data put
  there can still restyle the page (cover it with an overlay,
  hide or move what is on it) and load URLs (`background: url(https://elsewhere/?...)`,
  `@import`), which tells another site that the page was viewed and can carry what the
  style sheet says. Put only values you chose into styles (a colour from a list, a
  number), or check what users give against what you allow. A Content-Security-Policy
  header that keeps loading on the site (`img-src 'self'; font-src 'self'`, sent with
  `respond` or by a proxy in front) stops the beacons that go through `url()`.
- Code nested more than 256 levels deep (brackets, operators, elements, ...) is refused
  when it is parsed, so source from anywhere cannot use up the stack.
- `respond` and `redirect` refuse header values with line breaks, which would let text
  from a request add headers of its own, and a location that runs code or carries a page
  (`javascript:`, `vbscript:`, `data:`), naming the one given (`` `redirect` is not given
  a vbscript: URL ``), as `stream`, `site_headers` and `navigate` do.
- A failing request answers `internal error`; the message, with file and line, goes to
  the terminal (and to the page too with [`--watch`](#options-of-quillo-run)). A builtin's
  error is `internal error` to a visitor in the browser too, in production.
- A `json:` pattern matches only a body sent as JSON (or with no content type), not a
  `text/plain` one, which a page on another site could send.
- A page on another site cannot post a form here: a `POST`, `PUT`, `PATCH` or `DELETE`
  with a form, multipart or plain-text body that the browser marks as cross-site
  (`Sec-Fetch-Site`, or an `Origin` naming another host) gets a 403. JSON from another
  site needs the browser to ask first, which nothing here answers, and programs that
  call the site (webhooks, scripts) send no `Origin`, so both work as before. For a
  path that must take such posts, such as a payment provider's return or a single
  sign-on callback, say so at the top of the program: `allow_cross_site("/sso")`.

  ```quillo
  allow_cross_site("/payment")        // the provider's page posts back here

  route POST /payment/return { form: { order: str, status: str } } {
      // anyone can post this: check with the provider before believing it
      redirect(`/orders/${order}`)
  }

  route POST /notes { form: { text: str } } = "saved"
  ```

  A form on `https://pay.example` posting to `/payment/return` reaches the route; the same
  page posting to `/notes` gets 403 and the text `this site does not take form posts from
  other sites`.

  Nor can such a page open the WebSocket on which pages hear of live changes
  (`/_quillo/live`), which the browser would otherwise let it do.
- Globals read with `env`, `read_file`, the database or a service are never written
  into the browser bundle, nor are those made by a function that reads them
  (`let CONFIG = load_config()`); a component that uses one is a load-time error. Only
  variables named `PUBLIC_...`, read with the name written out, are.
- `http.*` in a component goes from the visitor's browser, never from the server, so it
  cannot reach the server's own network: while the server renders a component, a call
  from its code is an error before anything connects.
- `quillo check` warns about an `http.*` call on the server whose host comes from a
  value, which could let a visitor reach the server's own network (localhost, a private
  network, a cloud's metadata service), unless its options say `public_only` (see
  [HTTP client](#http-client)).
- `quillo check` warns about SQL put together from values (`${x}`, `+`, or
  `join([...])`) that reaches `db.query`, `db.one` or `db.exec`, or the same on a
  connection from `db.connect`, whether passed directly, piped (`` `...${x}` |
  db.query ``) or made first in a variable (`let q = ...`, `q += ...`); pass the values
  as `?` parameters instead. Names cannot be parameters: put a table's or a column's
  name in with `sql_name(name)` (or a list of them; quoted `"name"`, or `` `name` `` when
  the program's database is MySQL; `conn.sql_name(name)` quotes for the connection it is
  called on, `db.sql_name(name)` for the program's), and named parameters with
  `sql_param(name)` (or a list: `sql_param(["title", "body"])` is `:title, :body`) —
  both take only letters and digits of any script (`sql_name("名前")`), the marks that go
  on a letter (Hindi's vowel signs, an accent written apart) and `_`, not starting with a
  digit, at most 63 bytes, so that two names are two on every database, whatever their
  argument is made of (`` sql_name(`obj_${id}`) ``). SQL made of them, text as written (also kept in a
  variable), choices between those (`if`, `match`), joins of them with `+`, and calls of
  the program's own functions that only return such text
  (`` fn table_of(id: int) = sql_name(`obj_${id}`) ``), and a `map` whose function gives
  only such text on every way it returns, joined with `join`, is not warned about:
  `let order = match sort { "new" => "created desc", _ => "score desc" }` and then
  `"select * from posts order by " + order`. A run-time set of columns, from a list the
  program wrote: `` `insert into t (${sql_name(cols)}) values (${sql_param(cols)})` `` with
  a map of those columns as the parameters, or for an update the pairs, which every
  database takes (a column list, `set (a, b) = (...)`, is not MySQL's, nor PostgreSQL's for
  one column): `` let set = cols | map((c) => `${sql_name(c)} = ${sql_param(c)}`) | join(", ") ``
  and then `` `update t set ${set} where id = :id` ``. Only the builtins are trusted:
  a `sql_name`, `sql_param`, `map` or `join` the program defines, or a name the lambda
  binds again, is followed no further, and raw text in the lambda
  (`` keys(data) | map((c) => `${c} = 1`) ``) is warned of. Take the columns from
  the program, never from what the visitor sent: `keys(request.form)` names `is_admin`
  and `id` as well as `email`, which `sql_name` takes, as they are names, so a sign-up form
  with `is_admin=1` made an admin (FEEDBACK51.md, Q-662). A record type, `{ form: Signup }`,
  keeps its fields only.
  It is a warning, not a proof.
- Inside `db.transaction` on SQLite, `sleep`, `http` and queries to another database
  (PostgreSQL, MySQL) hold up other requests instead of letting them read and write
  inside the open transaction *(since 0.7: a query to another database let them in)*.
- **A transaction is `db.transaction`'s, not written by hand** *(since 0.7)*. `db.exec`,
  `db.query` and `db.one` refuse `begin`, `start transaction`, `commit`, `end`,
  `rollback`, `savepoint` and `release` alone: a `begin` whose `commit` was never reached
  (a `?` that left, a `reply 400`) left the shared connection in it, and every other
  request's writes with it. `db.transaction(() => ...)` commits, rolls back when something
  fails, and keeps other requests out. A script run whole (`db.exec(read_file("seed.sql"))`)
  may hold its own transaction, as `sqlite3 .dump` writes them, but one it leaves open is
  rolled back and is an error; a SQLite connection still in a transaction at the end of a
  request is rolled back too, and logged. `quillo check` says so of the literal text.

Values are immutable: `push`, `set` and `+` return new lists and maps. A map's keys are
text: `set(m, 5, x)` keys it `"5"`, and `m[5]`, `get(m, 5)` and `has(m, 5)` read that.
`set(xs, i, v)` gives a list with item `i` replaced (from the end when negative). A map
or list built in a loop with `m = set(m, k, v)` (or `push`, `merge`, `remove`), or as
`reduce`'s total with `(m, x) => set(m, ...)`, is changed in place, since nothing else
holds it: building one of 20,000 takes milliseconds. It is the same however it is
written, `m = m | set(k, v)`, `m = m.set(k, v)`, `xs += [x]` or `xs = xs + ys`, at a
script's top level too, and for what is inside: `m = set(m, k, get(m, k, []) | push(x))`
(a map of lists) and `g = set(g, y, set(g[y], x, v))` (a grid) change the inner list
where it is, with a key made by `str`, `lower`, `trim` or `int` too, as those give the same
written twice. `s += t` adds to the text where it is. Where something else still holds it
(a second name for it, say), each step copies it, so that name keeps what it had. In the
browser a list's `push` and `set` always copy, so build a long list there with `for`, or
`flatten` chunks. `flatten` takes one
level off: `flatten(flatten(xs))` for two.

## Paths and links

`read_file`, `write_file`, `delete_file`, `file`, `files`, `save_upload` and
`check_program` take a path from the program's folder, and stay in it. A path with `..`
or starting with `/` is refused, unless the program is started with [`--allow-path`](#options-of-quillo-run)
naming the folder it leads to (`quillo run main.qwe --allow-path /srv/shared/uploads`). A
path is judged where it really leads, with its links followed:

- A link inside the folder that leads inside it works as any path: `alias -> real`.
- A link that leads out of the folder is refused, as `..` is. A cloned repository or an
  unpacked archive can bring one (`data -> /home/other/...`), and on a shared server the
  folder it leads to may be another account's. The first one is said in the log, once:
  `quillo: data/s.txt leads out of the program's folder through a link, to
  /srv/outside/s.txt, and is refused; --allow-path FOLDER lets the program reach a
  folder outside`.
- A link that leads nowhere is refused too: writing through it would make its target,
  wherever that is.
- A program run through a link (a deploy's `current -> releases/42`) is inside the
  folder the link leads to, so its own files work as before. A main file that is itself
  a link (`app.qwe -> /srv/releases/42/main.qwe`) takes its `use`, `import`, `mount` and
  `service` files from beside the real file (a `./` path from the real file's folder),
  while its policy, `.env`, data, Quillo's own tables and messages stay at the link's
  folder, with the path as it was given.
- `--allow-path DIR` opens a folder outside on purpose (a path from the folder `quillo`
  runs in, taken as it really is): a deploy's `uploads -> /srv/shared/uploads` needs
  `--allow-path /srv/shared/uploads`.

What is refused is what any refused path is: an error value from `read_file`,
`write_file` and `delete_file` (`read_file: path must stay inside the program's folder:
data/s.txt`), a 404 from `file` and `files` (the same whether the folder outside is
there or not), and a runtime error from `save_upload`.

```quillo noexec
// alias -> real (inside), data -> ../outside, dangling -> ../outside/gone.txt
print(read_file("alias/in.txt"))                 // inner
print(read_file("data/s.txt") ?? "refused")      // refused
print(write_file("dangling", "x") | is_err)      // true
```

Only the path's parts are looked at, and only a path with a link in it is resolved, so a
static file costs what it did. A link changed between the check and the open is not
caught: Quillo does not use Linux's `openat2`.

## Files of a program

`use lib/forum.qwe` brings another file into the program, and its names into this file.
`import` *(since 0.7)* takes some of them. A file sees its own names, the builtins, and what it takes:

```quillo
// ui/thread.qwe
use lib/forum.qwe                         // every public name of the file
import lib/forum.qwe use *                // the same
import lib/forum.qwe use latest, Feed     // only these
import lib/forum.qwe use latest as newest // this one, called `newest` in this file
```

The file comes first, and the first word says what kind of thing it is, for three reasons:

1. **Mental model.** The line reads in the order things happen: what kind of code this is,
   where it is, then which of its names you want. `import` is Quillo or Rust on the
   server; `client`, `preload` and `preconnect` are the browser, and the word says how
   the page gets the script. One glance at the start of a line tells you which side of
   the program you are on, with nothing to read from the end of a long address.
2. **Editor.** Once the file is written the editor knows its names, so it can offer them
   as you type, and hide those you already took. With the names first, as `import { a }
   from "x"` has them, there is nothing to offer until the path is there. The word is
   `import` rather than `from` for the same reason: `from` starts lines of SQL, and
   programs use it as a name.
3. **Readability.** Lines line up by where things come from, so a file's sources read
   down the left. An address can be long, and the names no longer sit after it at the far
   right: `use` may take the next line.

0.6's `use latest, Feed from lib/forum.qwe` still works, with a warning; `quillo fix`
turns it around. A later release removes it.


- A `use` is for the file that writes it, and is not passed on: when `lib/a.qwe` `use`s
  `lib/b.qwe`, a file that `use`s `lib/a.qwe` sees `a`'s names and not `b`'s. The order
  of `use` lines does not matter, and files may `use` each other.
- **For now a name the file did not take still works, with a warning** *(since 0.7)*: "`price` comes
  from lib/money.qwe, which this file does not take: add `import lib/money.qwe use
  price`". `quillo fix main.qwe` writes those lines, in every file of the program
  (run it once for each program, a service's or a mounted one's too), after a byte-order
  mark and with the file's own line ends (CRLF in a CRLF file). A later release
  makes this an error.
- Two files may define one name: a `helper` in `lib/a.qwe` and a `helper` in `lib/b.qwe`
  are two functions, and each `fn`, `view`, `component` and top-level `let` works this
  way. A file's own name comes first, and then a name it took by name. A file that
  `use`s both files whole and calls `helper` is told to choose ("`helper` comes from
  lib/a.qwe and lib/b.qwe: take the one you mean: import lib/a.qwe use helper"). Taking a
  name the file defines, or one name from two places, is an error: `as` gives one another
  name.
- Three things stay the program's. A `server fn`'s name is its address, so it is defined
  once in a program, and a plain function of that name in another file gives way to it.
  A `type` and a `service` are defined once too, and every file sees them without a
  `use`.
- So a file makes sense only in its program: `quillo check main.qwe` checks it with
  the rest, and so does the editor (a file is checked as part of the program whose main
  file `use`s it; below). Go to definition finds where a name comes from.
- Within a file a name is defined once: a second `fn helper()`, `view` or `component` is
  an error that names both places. `let x` twice in one file gives the variable again.
  Two routes alike are not reported: the first declared
  answers. A function with a builtin's name is a warning (it replaces the builtin for
  the whole program); a variable may take one (`let count = 0`), a type's name included
  (`let num = 5`, `for str in names`): in `let`, `for` and parameters a name binds, and
  only in `match` does `int` test a type. But a *top-level* `let` named like a builtin
  (`let last = ...`, `let int = ...`) hides it in every file of the program: a warning
  at the `let`, and an error at each call that meant the builtin. The same goes for the
  modules: a variable called `db`, `http`, `json`, `re`, `math`, `bit`, `stats`, `vec`,
  `date`, `money`, `crypto`, `image`, `udp`, `channel` or `media` hides the module
  (`let json = request.json`, `let date = row.created`, a parameter `re`), so name such
  values otherwise (`body`, `created`).
- Paths are from the main file's folder, not from the file that writes them:
  `lib/b.qwe` writes `use "lib/c.qwe"`. A path that starts `./` or `../` is from the
  folder of the file that writes it, so a library's file can name its neighbour
  (`ui/slug.qwe` writes `import ./theme.qwe use cls`) and be checked alone, `quillo check
  ui/slug.qwe`. A path outside the folder works (`use "../shared/auth.qwe"` in the main
  file); there are no globs, and a `.qwer` file is `use`d the same way (see
  [Rust](#rust)) (FEEDBACK25.md, Q-325).
- A path may be written without quotes: `use lib/auth.qwe`, `use ../shared/auth.qwe`,
  and `use lib\auth.qwe`, its backslash read as `/`. Without an extension it is the
  `.qwe`, `.qwer` or `.rs` file there (`use lib/auth`), and an error when two are
  (`` `helpers` could be helpers.qwe or helpers.rs: write the one you mean with its
  extension ``); written with its extension, in any case (`MAIN.QWE`, `x.RS`), it is that
  file only, so `import lib.qwer use hi` with only `lib.qwe` there is an error. A file
  that is not there is one error at its import line, for every form, with a file near it
  (`cannot read lib/helpers.rs: No such file or directory (did you mean
  lib/helpers.qwe?)`), and its names are not said again as unknown. `import lib/auth.qwe
  use *` says the same as `use lib/auth.qwe`: every public name.
- A file `use`d twice, or by several files, runs once. Within one file, a second `use`
  of a file, or a name taken twice from it, is a warning: one is enough.
- A name taken `as` another is called that in the file: after `import lib/m.qwe use
  price as cost`, writing `price` is a warning ("this file takes `price` as `cost`"),
  and still works, as a name not taken does. Take both to have both: `use price as cost,
  price`. A type taken `as` another, `import lib/geo.qwe use Point as Pt`, is `Pt` in
  annotations, patterns and type definitions too; `as` cannot take the name of one of
  the program's types (`import lib.qwe use one as Point` is told `` `Point` is a type of
  the program: take `one` from lib.qwe under another name ``), and a name no type can be written as (`P`, one letter) is warned of
  with one that can (`Pt`).
- A `return` at the top level of a file ends the file there: nothing after it is
  defined, and `quillo check` says so at the first line after it (``this never runs: the
  `return` at line 2 leaves the file first, so nothing below it is defined``), as the
  program does when it runs.
- A `use` list may go on over lines, each line after a `,`. A `,` at the end of the
  list is an error: the next line is the next statement, not a name to take.
- **A file's text.** A file is UTF-8; one that is not is refused with the line and column
  of the first bad byte, by `run`, `check`, `build` and `classes` alike. A byte-order mark at the start is skipped, and so is a first line
  starting `#!`, in the main file and in a `use`d one, so a script with
  `#!/usr/bin/env quillo` runs as `./tool.qwe`. A CRLF is a line break, so a string over
  two lines is the same text from a Windows checkout (`\r` written as `\r` stays). A
  character that cannot be seen is named by its code: `unexpected character U+00A0 (a
  no-break space), which cannot be seen: delete it`.
- What runs in the browser, an address or a `.js` or `.mjs` file, starts with `client`:
  `client "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js" use Chart`
  (see [JavaScript libraries](#javascript-libraries)). `preload` in its place fetches the
  script from the page's `<head>`, and `preconnect` only connects to its host. `client
  module "..." use Chart` takes an export of an ES module, kept in the runtime and not on
  `window`; `use default as Name` and `use * as Name` take the default export and the
  whole module. A `.mjs` file is a module without saying so.
- **`private` and `public`.** `private` before a `fn`, `view`, `component` or top-level
  `let` keeps the name to its file: the file uses it as any name, other files do not
  see it, and two files may each have a private `helper` without clashing. `public`
  says the default, for a file that wants to say what it offers:

  ```quillo
  // lib/forum.qwe
  private let PAGE = 20
  private fn page_of(posts: list, n: int) -> list = posts | drop((n - 1) * PAGE) | take(PAGE)
  private view Post(p: map) = <article><h2>{p.title}</h2></article>

  public fn latest(n: int = 1) -> list = page_of(db.query("select * from posts order by id desc"), n)
  public view Feed(n: int = 1) = <div>{for p in latest(n) => <Post p={p}/>}</div>
  ```

  Another file calling `page_of` is told: "`page_of` is private to lib/forum.qwe". Only
  at a file's top level, and not for a `server fn` (an address on the server anyone can
  call; keep the work in a private `fn` it calls) or a `type`. A view's or component's
  parameter cannot share a private name of its file, as props are passed by name, nor a
  name two files define.
  Elsewhere `private` and `public` are ordinary names (`{ private: true }`). A private
  function given to `schedule` or `every` keeps its job to itself, under a name of its
  own (`tick__pv` and eight hex digits in the log), made from the file's path from the
  main file's folder, so copies of a program started from different folders (`quillo
  run current/main.qwe`, a service under `--split`) share the job.
- An editor finds the program a file belongs to by looking, in the file's folder and up
  to three above it (stopping at a folder with `.git` or `Cargo.toml`), for the first
  `.qwe` file by name that `use`s it, directly or through others. Keep a program's files
  within three folders of its main file, and one program per folder: two main files
  side by side that `use` one file have it checked as part of the first by name.
- A file that cannot be parsed is reported first, since what follows may be its doing,
  and the names it defines are not also reported as unknown where they are used.
- `x.helper()` finds a function by its plain name, so it does not reach a name two files
  define: write `helper(x)`.

## Decimals

A `dec` is an exact decimal number, for money and whatever else is counted in cents:
`12.50m`, `5m`, `-0.01m` (the `m` is C#'s). A float is a binary fraction, so `0.1 + 0.2`
is `0.30000000000000004`; decimals are exact, so `0.1m + 0.2m == 0.3m`.

```quillo
let price = 19.99m
let total = price * 3 + 4.50m          // 64.47m
let vat = round(total * 0.21m, 2)      // 13.54m (half to even)
let each = total / 3                   // 21.49m
dec("12.50")  dec(42)  dec(0.1)        // 12.50m, 42m, 0.1m
```

- A decimal keeps its digits: `12.50m` shows as "12.50", and `1.10m * 3` is `3.30m`.
  Equality and order do not see zeros at the end (`12.5m == 12.50m`), and neither do
  map keys (`set`, `get`, `group_by`).
- It holds 28 or 29 significant digits: a 96-bit whole number and up to 28 decimals, as
  C#'s `decimal`. `+`, `-` and `*` are exact; an answer with more decimals than fit keeps
  its whole part and rounds the rest half to even, and a whole part too big is an error
  (`decimal overflow`), as with ints. `/` is exact when the answer has 28 decimals or
  fewer, else rounded half to even to 28 or 29 digits (`1m / 3` is
  `0.3333333333333333333333333333m`). Dividing by zero is an error, and `%` is the
  remainder as for ints.
- With an int, a decimal stays exact and gives a decimal. With a float it is an error,
  never a silent float: `1.5m + 0.5` says to convert one with `dec(x)` or `float(x)`.
  `==` compares a decimal and a float through the float's shortest text (`0.1m == 0.1`).
- `dec(x)` makes one from text (exactly: `dec("abc")` is an error value, as `int("abc")`
  is; an exponent is read, so `dec("1e3")` is `1000m`), an int, or a float through its
  shortest text (`dec(0.1)` is `0.1m`, not
  `0.1000000000000000055…`). `int(d)` keeps the whole part, `float(d)` gives the
  nearest float, `str(d)` its digits.
- `round(d, digits = 0, mode = "half_even")` gives exactly that many decimals
  (`round(12.5m, 2)` is `12.50m`; negative digits round to tens, hundreds, ...). The
  modes: `half_even` (banker's rounding, the default), `half_up` (a half away from
  zero, as taught at school), `half_down`, `up` (away from zero), `down` (toward zero),
  `ceiling` and `floor`. `floor`, `ceil` and `abs` give decimals; `sum`, `min`, `max`
  and `sort` take lists of them, and a sum stays exact.
- `pow(d, n)` raises a decimal to an int's power, by squaring: exact while the answer
  fits, else each product rounded as `*` rounds, and a negative power is one over the
  positive one *(since 0.7)*. Interest and loan payments need no floats:
  `pow(1m + 0.05m / 12, 360)` is `4.4677443140061322124280701433m`, right to 25 digits. A
  power that is not an int is an error: `float(x)` and `math.pow` for that.
- `math`, `stats` and `vec` work on floats: a decimal there is an error that says to
  use `float(x)`.
- `json.stringify`, and a route answering a map, write a decimal as a string
  (`"12.50"`), so nothing that reads it rounds it. `json.parse` makes no decimals:
  numbers stay numbers.
- In a pattern or a record's field, `dec` takes a decimal, an int, numeric text
  (`"12.50"`, from a query, a form or JSON) or a JSON number (through its shortest
  text), and gives a decimal: `route GET /quote ? { amount: dec }`, `type Line = {
  price: dec }`, `/pay/:amount(dec)`. A `server fn`'s `dec` parameter takes the same
  from a page. In an annotation (`fn f(x: dec)`) it takes only a decimal.
- In the browser decimals are the same, and compiled components give the server's
  answers: arithmetic, comparison, rounding, display. They cross between the server
  and the page (props, state, server fns' arguments and answers) as decimals. Ints too
  large for a JavaScript number (past 2^53) reach the page whole, as BigInts, and show
  as on the server. A whole float is a float there too, so adding one to a decimal is an
  error on both sides, and `money.of(2.0, "USD")` is refused in both: an amount never goes
  through a float.
- `quillo check` knows them: `12.5m + 1` is a `dec`, `12.5m + 0.5` an error, and `round`,
  `abs`, `floor`, `ceil`, `pow`, `sum`, `min` and `max` keep the type they are given. `num`
  is still `int | float`.

**Rounding.** Each place that rounds says how. A tie, the 5 of `0.125` to two places:

| Where | A tie | `0.125` | `1.005` |
|---|---|---|---|
| `money.*`, and `round(d, n)` of a decimal | to even, unless a mode says otherwise | 0.12 | 1.00 |
| `format_number(x, locale, n)` | away from zero, from the shortest decimal, as `Intl.NumberFormat`; or `mode` | 0.13 | 1.01 |
| `round(x, n)` of a float | away from zero, from the float's shortest decimal *(since 0.7: `1.005` was `1.0`)* | 0.13 | 1.01 |
| `money.format`, where a currency shows fewer decimals than it has (IRR) | away from zero, as `Intl.NumberFormat` | | |
| PostgreSQL `numeric(p, s)` and MySQL `decimal` when a value has more decimals | away from zero | 0.13 | 1.01 |

So write a line of an invoice as money works it out: `format_number(x, locale, 2,
"half_even")`, or `money.format` of the line's money, never `format_number` of the
amount with its default, which may show a cent more than the total adds up.
- [Compiled numbers](#compiled-numbers) leave decimals to the interpreter.

**In databases.** PostgreSQL's `numeric` and MySQL's `decimal` come back as decimals with
their scale (`numeric(10, 2)` gives `19.99m`, and `12.50` stays `12.50m`), and decimals
are sent exactly. A value without a scale (as `sum` of ints gives) is an int, and one
past 28 decimals or 29 digits is its exact text. In MySQL a decimal parameter arrives
as text, which MySQL turns exactly into a `decimal` column, but into a double inside an
expression: write `cast(? as decimal(12, 2))` there.

SQLite has no decimals. A decimal is stored as its text, and a column declared
`decimal(12, 2)`, `numeric(12, 2)` or `money` is read back as decimals (a bare `numeric`,
SQLite's name for any number, gives numbers as before). But such a column has
NUMERIC affinity: SQLite turns "0.10" into the real 0.1 (and rounds past 15 digits), so
the scale is lost and large amounts are not exact. For exact amounts, declare the
column `decimal_text` (TEXT affinity: SQLite keeps the digits as written, and Quillo
still reads them as decimals), or keep whole minor units (`amount_cents integer`; see
`money.to_minor`). SQL's own sums and ordering do not work on text: `max` of a
`decimal_text` column is the largest text (`"9.99"` before `"100.00"`), as text; add up
and sort in Quillo, or keep minor units. The first time SQLite is given a `create table`
or `alter table` with a `money`, `decimal`, `dec(p, s)` or `numeric(p, s)` column, the log
says which column, once *(since 0.7)*.

PostgreSQL's own `money` type keeps a count of the smallest unit of its `lc_monetary`'s
currency. Quillo reads it with that currency's decimals, asked of the connection the first
time a result has one: `500` written reads back as `500m` under a yen `lc_monetary`, and
`500.00m` under a dollar one *(since 0.7: it was always read as cents, so ￥500 came back
as `5.00m`)*. It carries no currency: `numeric(p, s)` and a currency column say more.

A list given as a query's parameters, `db.exec("insert ... values (?)", [x])` as Python's
`sqlite3` takes them, is one parameter, stored as the list's JSON text. Parameters are
separate arguments: `db.exec(sql, a, b)`. `quillo check` says so when the SQL is written
out and has a `?` *(since 0.7)*, but not where a list is PostgreSQL's array: a `?` in
`any(...)` or `all(...)`, or cast to an array type (`?::int[]`, `?::jsonb[]`).

## Money {#standard-library-money}

`money` keeps amounts of a currency: a map `{ amount: dec, currency: "USD" }`, the
amount always with the currency's minor units (ISO 4217: JPY 0, USD 2, BHD 3). It is a
plain map, so JSON, the database and the browser take it as it is, and `money` checks
it each time it is given one. On the server and in the browser alike:

```quillo
let price = money.of(19.99m, "USD")             // { amount: 19.99m, currency: "USD" }
money.of(form.amount, "EUR") ?? reply 400       // text from a form; an error value if bad
money.parse("1.234,50 €", "EUR", "de-DE")       // 1234.50, as a German visitor writes it
money.of("12.345", "USD")                       // err: more decimals than USD has
money.of("12.345", "USD", "half_up")            // 12.35
money.from_minor(1250, "USD")                   // 12.50, from cents
money.to_minor(price)                           // 1999
price + tax  price - discount                   // amounts of one currency, added
money.add(a, b)  money.sub(a, b)  money.neg(a)  money.compare(a, b)   // -1, 0, 1
money.mul(price, 3)  money.mul(price, 0.075m, "half_up")              // rounded to cents
money.allocate(money.of(100, "USD"), [1, 1, 1])  // 33.34, 33.33, 33.33
money.format(price, "de-DE")                    // "19,99 $"
```

- `money.of(amount, currency, mode)` takes a decimal, an int or numeric text (a float
  is a runtime error, as is a currency that is not text: `dec(x)` it first). Text is an
  amount as a program writes one: digits, a sign and a point, not `"1e3"` and not
  `"1,234.50"`. An amount with more decimals than the currency has, bad text, an amount
  whose minor units do not fit an int (so that `money.to_minor` always can) and an
  unknown currency give error values, as `int("x")` does; with a rounding mode (those of
  `round`), the amount is rounded instead. Codes are ISO 4217's
  (`money.minor_units("JPY")` is 0), and withdrawn ones still read, so last year's
  invoice in leva is money: BGN, HRK, ANG, SLL, ZWL and the others withdrawn since 2007
  *(since 0.7)*. Codes without minor units, such as XAU, and crypto currencies are not
  money.
- `money.parse(text, currency, locale, mode)` reads an amount as the locale writes one,
  for a form *(since 0.7)*: its decimal mark and group mark, Indian grouping in en-IN, a
  minus, and the currency's symbol or code before or after it, as `money.format` writes
  them. Groups must be whole (`"1.00"` is not an amount in de-DE), and a mark of the
  other kind is refused (`"1,5"` is not an amount in en-US). `money.of` reads text as a
  program writes it, so a German visitor's `"1.000"`, one thousand, was one euro there.
  Without a locale, the program's (`locale()`), else en-US. What is wrong is an error
  value, as from `money.of`.
- `a + b` and `a - b` on two money values add and subtract the amounts, as `money.add` and
  `money.sub` do, with `+=` and `-=` too; two currencies give an error value *(since 0.7:
  `+` merged the two maps, so a cart's total was its last line)*. Only maps of exactly
  `amount` and `currency` are money here: other maps still merge. `money.add`, `sub` and
  `compare` take two amounts of one currency, and two currencies are a runtime error.
  `mul(m, factor, mode = "half_even")` multiplies by a decimal or an int and rounds to the
  currency; `round(m, mode)` rounds an amount made by hand. `zero(currency)`,
  `is_zero(m)`.
- Every `money` function reads a map it is given as `money.of` reads an amount: the code
  trimmed and in any case, text without an exponent, no more decimals than the currency
  has, minor units that fit an int. `money.round(m, mode)` alone takes more decimals, and
  makes money of them. Anything else is a runtime error that says why *(since 0.7: some
  functions rounded $1.005 and others took it as it was, so `compare` said it was more
  than $1.00 while `format` wrote $1.00)*.
- An amount past what `money.of` takes (minor units that do not fit an int) is an error
  value from `+`, `-`, `money.add`, `money.sub`, `money.mul`, `money.neg` and
  `money.round` *(since 0.7: they made it, and only `money.to_minor` refused it)*. A bad
  argument (a float, a map that is not money, two currencies to `money.add`) is a runtime
  error, a mistake in the program. `quillo check` types their results as money or an
  error.
- `money.allocate(m, ratios)` splits an amount by ratios (ints or decimals; a number n
  for n equal parts) without losing or making a minor unit: each share gets its part,
  rounded down, and the units left over go one each to the shares closest to their next
  unit, the largest remainders, a tie to the first *(since 0.7: to the first shares, so a
  1:99 split of $0.10 gave the 1% share $0.01)*. The shares always add up to the amount,
  and a ratio of 0 gets nothing.
- `money.format(m, locale)` writes it as CLDR has it for the locale, as
  `Intl.NumberFormat` writes it in a browser: `$1,234.50` (en-US), `1.234,50 €` (de-DE),
  `1 234,50 €` (fr-FR, a narrow no-break space between thousands), `￥1,235` (ja-JP),
  `₹12,34,567.00` (en-IN), `F CFA 1,234` (XOF in en-US), `IRR 1,235` (CLDR shows no
  decimals of the rial; the amount keeps them). Without a locale it uses the program's
  (`locale()`, with [Translations](#translations)), else en-US. 122 locales are covered,
  written from ICU 77's data by `scripts/money-tables.mjs`, and every currency's symbol in
  each *(since 0.7: 48 locales and 16 symbols)*. A name is matched without regard to case,
  with `_` for `-` and its extensions (`-u-...`) left out; Chinese by its script
  (`zh-Hant` as zh-TW); Spanish of Latin America as es-419 and Portuguese of Africa and
  Asia as pt-PT, as CLDR's parent locales are; a language alone as its first locale (`fr`
  as fr-FR, `es` as es-ES). Any other is written as the first of its language, else as
  en-US, and the log says so once (the browser's console too). Not covered: languages
  whose digits are not 0-9 (Arabic in Egypt, Persian, Bengali, Burmese, Nepali, Marathi),
  right-to-left ones (Hebrew, Arabic in the Emirates and Morocco), Urdu and en-150.
- `format_number(x, locale, digits, mode)` writes any number the same way: `format_number(
  1234567.5, "en-IN", 2)` is `"12,34,567.50"`. `digits` is kept within 0 to 28; a float
  too large for a decimal (past about 7.9e28), NaN or an infinity is written plainly,
  without grouping. It rounds a tie away from zero, from the number's shortest decimal, as
  `Intl.NumberFormat` does, on both sides: `2.5` to 0 digits is `3`, and `1.005` to 2 is
  `1.01`; `mode` rounds as `round` does instead (`"half_even"`, as money) *(since 0.7)*.
  `digits` may be `{ min, max }` (`max` is `max(min, 3)` when left out), for as many digits
  as the number needs within them: `format_number(10, "en-US", { max: 3 })` is `10`,
  `8.875` stays `8.875`, and `{ min: 2, max: 3 }` writes `8.8` as `8.80`. With no
  `digits`, every digit of the number is written (`Intl` stops at three), on both sides.
  A negative number rounded to zero keeps its minus, as `Intl` writes it: `-0.001` to 2
  digits is `-0.00`, and `-0.0` is `-0`. A whole float past 2^53 is written by its
  decimal digits in the browser too, from Quillo or from the page's JavaScript.
- There are no exchange rates: an amount stays in its currency, and converting is the
  program's arithmetic with a rate it chose.

To hold what a client sends to money's rules, name the type `money`: in a pattern, `route
POST /pay { json: { price: money } }`, a record's field, `type Line = { item: str, price:
money }`, or an annotation, `fn total(m: money)`. In a pattern or a field it takes a map
of an `amount` (a decimal, an int, numeric text or a JSON number) and a `currency`, holds
it to `money.of`'s rules with no rounding mode, and gives the money; a request that fails
gets a 400, and the log (or the page, under `--watch`) says which rule, without the value
sent: "`json.price` has more decimals than JPY has (0)". In an annotation it takes money
as the `money` functions take it. A record written out, `type Money = { amount: dec,
currency: str }`, checks only the shape *(since 0.7: that was the way, and $10.004 went
through to be cut to $10.00, `"1e3"` was 1,000, and BTC a 500)*.

## Numbers and data

`math` is Python's math module, `stats` its statistics module with what a data frame's
`describe` adds, and `vec` numpy's operations on lists of numbers. They are on the server
and in the browser alike, so a component can chart what a route computed:

```quillo
let totals = vec.column(orders, "total")          // [12.5, 30, 7.5, ...]
stats.describe(totals)        // { count, sum, mean, std, min, q1, median, q3, max }
stats.quantiles(totals, 10)   // deciles, as Python computes them
stats.correlation(ads, sales) // Pearson; "ranked" for Spearman
stats.linear_regression(ads, sales)   // { slope, intercept }
stats.histogram(totals, 20)   // { counts, edges }, as numpy's
vec.rolling_mean(daily, 7)    // a week's moving average
math.comb(52, 5)  math.log(8, 2)  math.fsum(xs)  math.isclose(a, b)  math.pi
```

- **`math`:** `sqrt cbrt exp exp2 expm1 log(x, base) log2 log10 log1p pow`, `sin cos tan
  asin acos atan atan2` and their hyperbolic forms, `degrees radians hypot dist`, `floor
  ceil trunc` (ints, up to 2^63; of an int divided by an int, written in the call, the
  exact int, as `floor` gives it) `fabs copysign fmod remainder modf frexp ldexp`, `factorial gcd lcm
  comb perm isqrt` (ints), `isclose isfinite isinf isnan`, `prod fsum sumprod`, `erf erfc
  gamma lgamma`, and `pi e tau inf nan`. What Python raises is an error here too: a math
  domain error (`math.sqrt(-1)`, `math.log(0)`), a range error (`math.exp(1000)`), an int
  past 64 bits (`math.factorial(21)`).
- **`bit`:** *(since 0.7)* `and(a, b) or(a, b) xor(a, b) not(a) shl(a, n) shr(a, n)`, on the 64 bits of
  an int, with the same answers in the browser (where JavaScript's own `&` and `<<` keep
  32). `shl` drops what goes past the top, unlike `*`, which refuses; `shr` keeps the
  sign, and `bit.and` masks it off: `bit.and(bit.shr(x, 8), 0xFF)`. A shift outside 0 to
  63 is an error. A variable called `bit` hides the module, as with `math`.
- **`stats`:** `mean fmean(data, weights) geometric_mean harmonic_mean`, `median
  median_low median_high mode multimode`, `pvariance variance pstdev stdev` (each with an
  optional known mean), `quantiles(data, n = 4, method = "exclusive")` (n up to 10,000), `covariance
  correlation(x, y, method = "linear") linear_regression(x, y, proportional = false)`,
  `normal_pdf normal_cdf normal_inv` (x, mu = 0, sigma = 1); and, beyond Python's,
  `percentile(data, p)` (p from 0 to 100, or a list of them; numpy's linear rule),
  `describe`, `histogram(data, bins = 10)` (or the edges as a list), `zscore`,
  `normalize` (to 0..1) and `rank` (ties share their average rank, a float; others are
  ints). What puts data in order or counts them (the medians, `quantiles`, `percentile`,
  `describe`, `rank`, `histogram` and its edges) refuses NaN and infinities, naming which
  and where, as `sort` does *(since 0.7: `median([3, NaN, 1])` was 3.0, and a histogram
  with `inf` counted nothing)*; a mean or a spread has them as Python's does. Statistics of no data, or of too little, is an error, as in Python, except
  `describe([])`, which is `{ count: 0, sum: 0 }` with the rest `null`, and `zscore` and
  `normalize` of a constant list, which give zeros. `mode` and `multimode` take any
  values, text too. Python's `median_grouped` and the rest of `NormalDist` are not
  there.
- **`vec`:** `add sub mul div mod` (two lists of one length, or a list and a number; ints stay
  ints, as with the operators), `pow dot norm(xs, p = 2)`, `cumsum cumprod diff`,
  `argmin argmax argsort(xs, descending = false) clip(xs, lo, hi)` (either bound may be
  `null`), `abs sqrt exp log round(xs, digits)`, `linspace(start, stop, n = 50, endpoint
  = true) arange(start, stop, step) zeros ones full(n, x)` (at most ten million items),
  `rolling_mean rolling_sum(xs, window)` (one per full window),
  `matmul(a, b)` (rows times rows, or times a list), `transpose`, and `column(rows, key)`
  (a field of each map, or an item of each list).

Answers are Python's: sums are compensated and rounded once, so `stats.mean`,
`stats.variance` and `math.fsum` give Python's exact results (its statistics module
uses fractions; this gets the same float without them), and the rest agree to the last
digit or one or two places from it. None of it runs in the interpreter: a list is read
once into floats, and a long one (over 128K items) is worked on by all the cores, in
chunks of 32K whatever the machine, so the answer does not depend on how many cores
it has. A median, a percentile or a quantile sorts only the part of the list where it
falls (cut points from a sample split the list into ranges of about equal size), and
`vec.argsort`, and `sort` of a list of 64 or more items that are all ints or all floats
(no NaN), sort those ranges side by side; any other list sorts on one core. There is
no matrix or data-frame type: rows are lists of maps, `vec.column` takes a field of
each, and `vec.matmul` and `vec.transpose` work on lists of lists. On a million floats, against
Python 3.9 (no numpy) on the same 10-core machine:

| | Quillo | Python |
|---|---:|---:|
| `stats.mean` | 0.35 ms | 214 ms |
| `stats.stdev` | 0.65 ms | 674 ms |
| `stats.median` | 1.3 ms | 44 ms |
| `stats.quantiles(xs, 100)` | 2.0 ms | 45 ms |
| `stats.correlation` | 1.4 ms | 246 ms (3.10's algorithm) |
| `stats.linear_regression` | 1.3 ms | 182 ms (3.10's algorithm) |
| `stats.describe` | 1.7 ms | 764 ms (by hand) |
| `stats.histogram(xs, 50)` | 0.6 ms | 126 ms (a loop) |
| `math.fsum` | 1.6 ms | 20 ms |
| `vec.dot` | 0.9 ms | 11 ms (`sum(map(mul, x, y))`) |
| `vec.add` | 1.9 ms | 20 ms (`list(map(add, x, y))`) |
| `vec.rolling_mean(xs, 30)` | 2.0 ms | 69 ms (a running sum) |
| `vec.argsort` | 5.4 ms | 75 ms (`sorted(range(n), key=...)`) |
| `sort` | 4.6 ms | 46 ms |

Making a list one item at a time is the interpreter's work, though: `map(range(n), (i)
=> ...)` over a million takes about 1.4 times Python's list comprehension. Built with
`vec` instead (`vec.div(vec.mod(vec.mul(vec.arange(n), k), m), 1000.0)`), the same list
takes a fifteenth of the comprehension's time.

## Dates and times

There is no date type: a date is its ISO 8601 text, `"2026-10-27"`, and a date-time is
RFC 3339 text with its offset, `"2026-10-27T14:30:00Z"` or `"2026-10-27T15:30:00+01:00"`.
Dates sort and compare in time order as text. Date-times do only when they are written
the same way, with the same offset and the same digits: `"…14:30:00.000Z"`, as
JavaScript's `toISOString()` writes it, sorts before `"…14:30:00Z"`, though they are
one instant. To order or compare date-times, use `date.compare(a, b)` or `date.ms`.
Both go through JSON, databases, forms and the browser as they are. The `date` module works on it, on the server and in the browser
alike, with the same answers:

```quillo
date.today()                          // "2026-10-27", in UTC; date.today("Europe/Paris")
date.now()                            // "2026-10-27T14:30:00Z", to the second
date.add_months("2026-01-31", 1)      // "2026-02-28": the end of the month, not March
date.to_zone("2026-10-27T14:30:00Z", "Europe/Paris")       // "2026-10-27T15:30:00+01:00"
date.at("2026-10-27", "17:00", "America/New_York")         // "2026-10-27T17:00:00-04:00"
date.at("2026-10-27T17:00", "America/New_York")            // the same, from <input type="datetime-local">
date.compare("2026-10-27T14:30:00.000Z", "2026-10-27T15:30:00+01:00")   // 0: one instant
date.days_between("2026-11-05", "2026-11-14")              // 9: the nights of a stay
date.format("2026-10-27", { locale: "de-DE", style: "long" })  // "27. Oktober 2026"
date.parse("27/10/2026", "DD/MM/YYYY")                     // "2026-10-27", or an error value
```

- **Making and reading:** `date.of(y, m, d)`, `date.valid(s)` (a date or a date-time;
  `date.valid(s, "date")` only a date and `date.valid(s, "date-time")` only a date-time:
  check text from a page with the first before the functions below, which take only
  dates and fail on a date-time with a runtime error, not an error value; or give the
  field or parameter the type `date`, see [Records](#records)),
  `date.parts(d)` (`{ year, month, day, weekday }`, weekday 1 for Monday to 7 for Sunday;
  a date-time adds `hour minute second offset`), `date.weekday(d)`, `date.is_leap_year(year
  or d)`, `date.days_in_month(d)` or `(year, month)`.
- **Arithmetic:** `date.add_days(d, n)`, `date.add_months(d, n)` and `date.add_years(d, n)`
  (a day past the month's end is its last day: `2024-02-29` plus a year is `2025-02-28`),
  `date.days_between(a, b)` (`b - a`, in days), `date.months_between(a, b)` (whole months:
  `2026-01-31` to `2026-02-28` is one), `date.start_of_month(d)`, `date.end_of_month(d)`.
  These take dates; `date.date_in(t, tz)` gives a date-time's date.
- **Milliseconds:** `date.ms(t)` is a date-time (or a date, at its midnight in UTC) as
  milliseconds since 1970, what `now()` gives, and `date.from_ms(ms, tz)` a time kept that
  way as a date-time (to the second): `date.from_ms(now(), "Europe/Lisbon")`.
- **Comparing:** `date.compare(a, b)` *(since 0.7)* is -1, 0 or 1 by the instants, fractions of a
  second included, a date counting as its midnight in UTC: the safe way to order or
  compare date-times written differently, `sort_by(xs, (x) => date.ms(x.at))` the way
  to sort them.
- **Time zones:** `date.valid_zone(z)` *(since 0.7)* asks whether a zone exists, for one a person typed;
  a zone it does not know stops the call, so `date.to_zone(t, z) ?? t` guards it too.
  `date.to_zone(t, tz)` is the same instant at the zone's offset then
  (daylight saving included), a fraction of a second that is all zeros dropped (`.000Z`
  becomes `Z`); `date.date_in(t, tz)` its calendar date there;
  `date.at(d, "HH:MM", tz, options)` the instant a local time names, and
  `date.at("2026-10-27T14:30", tz, options)` the same from one text (seconds may follow,
  and a space may stand for the `T`), as `<input type="datetime-local">` gives it. When
  clocks go back, a time that happens twice is the first, or `{ repeated: "later" }` the
  second; when they go forward, a skipped time is moved on by the gap (02:30 becoming
  03:30), or `{ skipped: "earlier" }` moved back, or `{ skipped: "error" }` an error
  value: `date.at: 2026-03-08 02:30 is skipped in that zone, as its clocks move
  forward`. A zone is a name of the IANA database, a zone or a link, in any case
  (`"Europe/Paris"`, `"US/Pacific"`, `"EST"`), `"UTC"`, or an offset as `date.parse` reads
  one (`"+05:30"`, `"+0530"`, `"+05"`, up to `+23:59`); left out, it is UTC. An offset of
  zero is written `Z`. The server and the browser take the same zones and refuse the same
  names, as an unknown time zone: `posixrules`, `Factory`, `Etc/Unknown`, `localtime`,
  `posix/...` and `right/...` (copies some systems keep), names the database dropped
  (`US/Pacific-New`, `Canada/East-Saskatchewan`, `SystemV/...`), Java's three-letter ids
  (`PST`), and any other name with a sign (`+5`, `+0560`, a U+2212 minus). The server
  reads the system's time zone database, and a copy built into `quillo` where there is none
  (Windows, small containers) and for a name the system's leaves out (Ubuntu 24.04 moved
  `EST`, `US/Pacific`, `Asia/Calcutta` and `EST5EDT` to a package of their own,
  `tzdata-legacy`), so a program takes the same zones on every machine; the browser asks
  its own (`Intl`).
- **Text:** `date.format(d, pattern)` with `YYYY YY`, `MM M` (month number, padded or
  not), `MMMM MMM` (its name), `DD D`, `dddd ddd` (the weekday's name), `HH H` and `hh h`
  (24 or 12 hours), `mm ss A` (AM/PM) and `Z` (the offset); anything else, and text in
  `[brackets]`, is kept as it is. A date-time shows its time as written: `date.to_zone` it
  first to show another zone's. `date.format(d, { locale, style })` gives a locale's
  `"short"`, `"medium"` (the default), `"long"` or `"full"` date, and `{ locale, pattern }`
  the pattern with that language's names. The locales are English, German, French,
  Spanish, Italian, Dutch and Portuguese, and the regions whose styles CLDR gives their
  own: `en-GB`, `en-CA` (`2026-09-05` short), `en-IE`, `en-AU`, `en-NZ`, `en-IN`,
  `en-ZA`, `de-AT` (`Jänner`, `Jän.`), `fr-CA`, `fr-CH`, `es-MX`, `it-CH`, `nl-BE` and
  `pt-PT`, the same on the server and in the browser (CLDR's forms, names in a pattern
  too); another region takes its language's (`de-CH` is German's), and any other
  language gives English. `date.parse(s)` reads a date or a date-time, with `T` or
  a space and an offset (`+01`, `+0100`, `+01:00` or `Z`), and gives it back as above;
  a date-time with no offset is an error value that says to read it with `date.at(v,
  zone)`, as it names no instant until a zone is given; an HTTP date (`Tue, 27 Oct 2026
  14:30:00 GMT`) is read as UTC;
  `date.parse(s, pattern)` reads by a pattern (English names; `YY` is 2000 to 2099; with a
  time and no `Z`, UTC; its `Z` reads `Z`, `+01:00` or `+0100`, not `+01`). Either gives
  an error value when the text is not a date. There is no token for fractions of a
  second, though `date.parse` reads them and `to_zone` keeps them.

A malformed date (`"2026-02-30"`, `"27/10/2026"`, a number) is an error in every function
but `date.parse` and `date.valid`. Years are 1 to 9999. `date.today` and `date.now` read
the clock, so they are the `time` effect, and scripts run with `run_script` have the rest
of `date` but not them. A program's own variable called `date` hides the module, as with
the others.

What a form's date and time inputs send, and what reads it:

| Input | Sends | Read it with |
|---|---|---|
| `<input type="date">` | `2026-10-27` | as it is: it is a date |
| `<input type="time">` | `14:30` | `date.at(day, v, zone)`'s second argument |
| `<input type="datetime-local">` | `2026-10-27T14:30` | `date.at(v, zone)` |
| `<input type="month">` | `2026-10` | no `date` function reads it: split it, `date.of(y, m, 1)` |
| `<input type="week">` | `2026-W44` | no `date` function reads it |

None of them carries a zone: the program says which (a user's setting, the `tz` cookie
below).

Business days, holidays and the day counts of interest are a program's own: a calendar
is a list of holidays and the arithmetic above.

The server does not know a visitor's time zone: `date.today()` is UTC's. A page tells it
with a cookie, set from `Intl.DateTimeFormat().resolvedOptions().timeZone` through
`js`, which a helper reads with `cookie("tz")`; the first page a browser asks for is made
before the cookie exists.

```quillo
fn zone() -> str = cookie("tz") ?? "UTC"     // in any function the request runs

server fn set_zone(zone: str) =
    date.valid_zone(zone) ? set_cookie("tz", zone, { max_age: "1y" }) : null

component SendZone(known) {            // once in the layout: sends the zone when it differs
    <span hidden onmount={() => {
        let z = js("Intl.DateTimeFormat", []).resolvedOptions().timeZone
        if z != known { set_zone(z) }
    }}></span>
}

route GET / = <html><body>
    <SendZone known={cookie("tz")}/>
    <p>Today, where you are: {date.today(zone())} ({zone()})</p>
    <form method="post" action="/meet">
        <input type="datetime-local" name="at"/>
        <button>Book</button>
    </form>
</body></html>

route POST /meet { form: { at: str } } = `Booked for ${date.at(at, zone())}`
```

In Manila the first page says `(UTC)`; from the next one on, `Today, where you are:
2026-11-02 (Asia/Manila)`, and 09:30 booked in the form is `Booked for
2026-11-20T09:30:00+08:00`. `set_zone` checks the zone, since anyone can call it with any
text.

## Database {#standard-library-database}

`db` talks to SQLite (built in), PostgreSQL or MySQL/MariaDB, with the same functions.
SQL problems are error values, so `?` and `??` work on them. `db` is a library, not
syntax: other stores (Redis, MongoDB, ...) come in through Rust (see
[Other stores](#other-stores)).

```quillo
db.open("app.db")                                   // SQLite, next to the program; ":memory:" also works
db.open("postgres://user:pass@host:5432/app")       // PostgreSQL
db.open("mysql://user:pass@host:3306/app")          // MySQL or MariaDB (also mariadb://)
db.exec("create table if not exists todos (id serial primary key, text text)")
db.exec("insert into todos (text) values (?)", text)       // { changes, last_id }
db.exec_many("insert into todos (text) values (?)", [["a"], ["b"]])   // { changes: 2 }, all or none
db.query("select * from todos where id > ?", 10)           // a list of maps
db.one("select * from todos where id = :id", { id })       // a map, or null; keys the SQL does not use are ignored
db.one("insert into todos (text) values (?) returning id", text)
db.transaction(() => { ... })                              // all or nothing; nests
db.transaction({ isolation: "serializable", retries: 3 }, () => { ... })
```

Without `db.open`, the database is `$QUILLO_DB` (a path or a URL), or else
SQLite in memory. A transaction rolls back if its function fails, returns an error
value or replies early, and also when a statement in it gives an error that nothing
takes: `db.exec(...)` alone on a line that fails rolls the whole transaction back, and
the transaction gives that error as its value, so half the work is never committed.
A transaction SQLite ends by itself (a full disk, an I/O error), rolling back all it held,
is over: each statement after it in the function is an error, and the transaction gives
``the database ended this transaction after an error, and rolled back all it held``
*(since 0.7: what ran after it was committed statement by statement)*.
An inner transaction whose error is dropped rolls the outer one back too; `let _ =
db.transaction(...)` for the inner one says its failure is not wanted, and the outer one
goes on. `quillo check` warns when a `db.transaction(...)` call's value is dropped
(FEEDBACK27.md, Q-332). A SQLite path is from the program's folder, not the one Quillo
runs in, and `quillo run` says which file it opened when it starts
(`quillo: database /srv/app/app.db`). Only `postgres://`, `postgresql://`, `mysql://` and
`mariadb://` are servers: any other text, `sqlserver://...` included, is taken for a
SQLite file's path. SQL Server, Oracle and the rest are reached through Rust (see
[Other stores](#other-stores)).

**Many rows at once: `db.exec_many(sql, rows)`.** *(since 0.7)* It runs the statement for each row, a
list of its `?` values, and gives `{ changes }`, the rows changed in all. It is all or
none: it runs in a transaction of its own, or as a savepoint of the one it is in, so a row
that fails (a duplicate key) leaves the table as it was, and gives the error. An empty
list changes nothing. On PostgreSQL and MySQL an `insert into t (...) values (?, ...)`
with nothing after its values goes as inserts of up to 1,000 rows each, as Python's
`executemany` sends them, not a round trip a row; any other statement (an `update`, an
insert with `on conflict`) runs a row at a time, in the one call. For one row's
`last_id`, use `db.exec`.

```quillo
let rows = for p in imported => [p.sku, p.name, p.price]
let r = db.exec_many("insert into products (sku, name, price) values (?, ?, ?)", rows)
if is_err(r) { reply 400 `Not imported: ${r.error}` }
print(`${r.changes} products`)
```

**A `db.open` that fails as the program starts, its error taken by nothing, stops it**
*(since 0.7)*, with the reason: ``db.open: error connecting to server: Connection refused:
the program's database cannot be opened, so the program does not start``. That is
`db.open(...)` alone on a line, or given back by a function called so. It ran on SQLite in
memory before, answered writes, and lost them at the next start; a wrong password, a server
down or a damaged file did it. An error the program takes (`let r = db.open(...)`, `??`,
`| is_err`) is a value, as is one from a `db.open` later, which leaves the database as it
was.

**Opening another database while it runs.** `db.open` after the start replaces the
program's database. Open pages showing its rows are told at once, every tenant's (the
database is the program's, whichever request opened it), and ask again; a live call is
not answered from what was read in the old one.

**What the calls give.** `db.query` gives every row, all of it in memory (there is no
cursor: page with `limit` and keyset or `offset`); `db.one` the first row or `null` (on
PostgreSQL it fetches the whole answer and keeps one: write `limit 1`); `db.exec`
`{ changes, last_id }`. A row is a map from column names, so two columns of one name
(`select a.id, b.id`) leave the last: name them apart with `as`. SQL `NULL` is `null`.
The two numbers mean the same on every database. `changes` is the rows the statement
matched: an `update` that sets a value a row already has counts that row (MySQL's own
count, of rows changed, is `?client_found_rows=false` in its URL), an upsert's or a
`replace`'s row counts once, a read and a schema statement count 0, and a script's is its
last statement's. `last_id` is the id of the one row an `insert` made (on SQLite and
MySQL), and `null` after anything else: an `insert` of several rows, an upsert that
updated, an `update`. PostgreSQL has no such id, so it is always `null` there: write
`insert ... returning id` with `db.one`, or `db.query` for several rows. SQL with no
statement in it (`""`, only spaces or comments) is an error value, on all three, from
`db.exec`, `db.query` and `db.one` alike.

**One statement, or several.** With values, `db.exec` runs one statement: on SQLite what
follows the first `;` is ignored, on PostgreSQL it is an error. Without values the whole
text runs, statement by statement (a seed file: `db.exec(read_file("seed.sql"))`), and
`changes` is the last one's. The text is split where the database would split it: a
function's, procedure's or trigger's body (`$$ ... $$`, `begin ... end`), a `do` block and
`begin atomic` are one statement with the rest, never a transaction begun by hand.
`db.exec` of a statement that gives rows (`insert ... returning id`) runs it, on SQLite
too; its rows are `db.one`'s and `db.query`'s.

**Parameters.** `?` takes values in order, as many as there are `?` (a different count
is an error, and so are placeholders given no values, on every database), or `:name`
takes them from one map (a name needs a value; other keys are ignored). One map is the
values by name only where the SQL has names: given to SQL with none, it is no value, so
a query put together from optional parts can always pass the same map; given for a lone
`?`, it is that value (a map stored as JSON). SQLite also reads `@name`, `$name` and
`#name`, and numbered ones (`?1`, `:1`), taking `name` from the map; on MySQL `@name` is a
variable of the session, so a map with that name is refused (`` `@x` is a variable of
MySQL's session (null unless set), not one of the values: write `:x` ``). `?NNN` is the
NNN-th value everywhere. PostgreSQL's `$1` turns the `?` translation off for that
statement, so `$1` and `?` do not mix; a `?` right after a word that is not SQL's
(`interval ?`, `date ?`) is taken for jsonb's operator and left alone: write
`?::interval`. PostgreSQL's `?|`, `?&` and `@?` are operators, `?||` is a value then
`||`, a `:name` inside brackets is a value unless it is a slice's (`a[1:n]`), and block
comments nest, as the server reads them. At most 32,766 values in a statement on SQLite,
65,535 on the others.

A value is what it is on every database: an error value given as a parameter is not
stored, but given back as the error it was (`value 2 cannot be stored: got error (it
failed before this call, ...)`), so `int("4O")` never becomes a row; a function is
refused. NaN and the infinities are refused, naming them (`value 1 is NaN, which cannot
be stored`), since SQLite keeps NaN as null and MySQL refuses both; `-0.0` is stored and
read as `0.0`, as SQLite and MySQL cannot keep it. A `||` joins text on all three: on
MariaDB, whose `||` is `or` by default, the driver sets `PIPES_AS_CONCAT` for SQL that has
one, migrations and scripts too.

**Bytes and other types.** There is no bytes value: a SQLite BLOB or a MySQL BLOB
comes back as text (bytes that are not UTF-8 replaced), and PostgreSQL's `bytea` as
`\x...` hex. Store base64 or hex text, or files beside the database.

**What failed.** `db.error_kind(e)` says what kind of failure a database's error value
is, the same for SQLite, PostgreSQL and MySQL, whose words for it differ: `"unique"` (a key
or unique column has the value already), `"foreign_key"`, `"not_null"`, `"check"`,
`"conflict"` (another transaction: the database busy, a deadlock, a serialization
failure), `"connection"` (the server ended the connection, or it could not be made:
`57P01` and class `08`) *(since 0.7)*, `"read_only"` (a write the database refused because
the transaction only reads, as a tool's that only reads is: `25006`, SQLite's "attempt to
write a readonly database", MySQL's 1792; FEEDBACK56.md, Q-858), or null for any other
failure and for anything else.
A transaction whose first statement finds that the server ended its pooled connection
while it sat idle, as after a restart or a failover, is begun again on another one: nothing
had run. It needs no effect.
Every database failure is an error value whose text starts `db: `, with the database's
own message (PostgreSQL's `detail` in brackets, then its SQLSTATE in square brackets:
`db: duplicate key value violates unique constraint "seats_pkey" (Key (k)=(1) already
exists.) [23505]`). `db.error_kind` reads PostgreSQL's code first (`23505`, `23503`,
`23502`, `23514`, and `40001`, `40P01` and `55P03` for a conflict), so it gives the same
word on a server whose messages are in German. MySQL's codes are in its text already.

```quillo
let r = db.exec("insert into seats (show, seat) values (?, ?)", show, seat)
if db.error_kind(r) == "unique" { return err("Someone has just taken that seat") }
r?
```

**Isolation and retries.** `db.transaction(options, f)` takes the isolation level and
how many times to run `f` again when the database gives up on it because of another
transaction:

```quillo
fn withdraw(account: int, cents: int) -> map with db =
    db.transaction({ isolation: "serializable", retries: 5 }, () => {
        let balance = db.one("select balance from accounts where id = ?", account).balance
        if balance < cents { return err("not enough money") }
        db.exec("update accounts set balance = balance - ? where id = ?", cents, account)?
    })
```

- Those are its two options; another is refused, when it runs and in `quillo check`
  (`` `db.transaction` has no option `isolaton` (did you mean `isolation`?) ``), where a
  misspelt key ran the transaction as if none were given.
- `isolation` is `"serializable"`, `"repeatable_read"` or `"read_committed"`; left out,
  it is the database's default (read committed on PostgreSQL, repeatable read on MySQL).
  PostgreSQL begins with `begin isolation level ...`, MySQL runs `set transaction
  isolation level ...` before `begin`. SQLite lets one transaction write at a time, so
  its transactions are serializable whatever is asked; with options it begins `begin
  immediate`, taking the write lock at once (another process holding it is then a
  conflict, tried again, rather than a failure half way). Without options a SQLite
  transaction is a savepoint that takes the lock when it first writes, and a busy
  database is not tried again.
- On PostgreSQL and MySQL a transaction waits up to 30 s for a free connection, then
  fails (`no PostgreSQL connection free after 30 s`).
- `retries` (0 to 20; 0 by default) runs the whole function again after a serialization
  failure or a deadlock: PostgreSQL's SQLSTATE 40001 and 40P01, MySQL's 1213 (deadlock),
  1205 (lock wait timeout), 1020 (a row changed since the snapshot, MariaDB) and 1412 (a
  table made since the transaction began), SQLite's busy database. Each try waits a little longer
  than the last (about 10 ms, doubling up to 320 ms, give or take half, so the two that
  collided do not collide again). Any other failure ends the transaction at once. Out of
  tries, the transaction gives the database's error value.
- After such a failure the transaction is over, whatever the function does with the
  error value: PostgreSQL refuses its next statements, and MySQL has already rolled it
  back (statements after it would otherwise run outside any transaction). So they fail
  at once, and a function that carries on anyway is not committed, but tried again.
- A transaction inside another is a savepoint, and cannot change the isolation: asking
  is an error. Only the outermost one tries again; give it the options.
- The function may run more than once, so it should do nothing that cannot be taken
  back: no `http.*`, `udp.send` or `channel.send` in it (send them after the
  transaction; `quillo check` warns), and global variables it changes are not rolled
  back. What it does in the database is.
- Under serializable isolation the database finds the conflicts itself: two
  withdrawals that read the same balance cannot both commit. Under read committed (or
  for a hot row, to wait instead of failing and trying again), lock what is read before
  writing it: `select balance from accounts where id = ? for update` (PostgreSQL,
  MySQL; SQLite has no `for update`, and needs none). A balance kept as a running total
  is updated in place: `update accounts set balance = balance - ? where id = ? and
  balance >= ?`, checking `changes`.

**Migrations.** `db.migrate("migrations")` (the folder is optional; that is its default)
brings the schema up to date from SQL files applied in name order: `001_todos.sql`,
`002_tags.sql`, and so on. Each file runs once, in its own transaction, and is recorded
in a table `_quillo_migrations` with its SHA-256. A file that fails is rolled back and
stops the program from starting, with the file's name and the database's message; a
recorded file whose text changed later gets a warning, not a second run. Servers that
start together take turns (an advisory lock on Postgres, an immediate transaction on
SQLite), so each file still runs once. Only files ending `.sql` directly in the folder
count (no subfolders); they sort by name as text, so `10_x.sql` comes before `2_x.sql`:
pad the numbers. The name is the key: a renamed file runs again. A file holds no
`begin` or `commit` of its own. The folder must exist: a missing one is an error that
says why it may be gone from a clone (`there is no such folder (git keeps no empty
folder: put the first migration in it, or a .gitkeep, which is not read)`). The table is `_quillo_migrations
(name, sha256, applied_at)`. Call it right after `db.open`:

```quillo
db.open(env("DATABASE_URL"))
db.migrate()
```

**SQLite** runs inside the server process, with foreign keys on and WAL mode, and waits
up to 5 s for a lock before a write is `"conflict"`. It is SQLite 3.46 with JSON, FTS5
full-text search and R*Tree, and without math functions (`sin`, `acos`, `radians`):
compute those in Quillo. Values: a column declared `bool` or `boolean` gives `true` and
`false`, other 0s and 1s are ints; `decimal(...)` and friends give decimals (see
[Decimals](#decimals)); booleans are stored as 0 and 1, lists and maps as JSON text;
dates are what text you store. There is one connection per database per process, and its
queries run under the interpreter's lock, on the loop's thread: a SQLite query holds up
the other requests of its instance while it runs. Several processes (a program started
with [`--instances`](#options-of-quillo-run) or [`--split`](#options-of-quillo-run), `quillo run main.qwe --instances 4`; another program) may share a file: readers do not wait, writers take turns.
There is no SQLCipher (no encryption at rest), and a file is opened by path, not as a
`file:` URI (so no read-only mode). `files()` and `quillo serve` never send a SQLite
file, whatever its name or folder (see [Static files](#static-files)); `file("app.db")`
sends one on purpose. Back it up with `db.exec("vacuum into
'backup.db'")` or `sqlite3 app.db ".backup ..."`, not by copying the file while it runs.

**PostgreSQL:**

- Placeholders can be `?`, `:name` (with a map) or Postgres's own `$1`; the first two
  are translated, skipping quotes (`U&"..."` names too), comments and `$$` strings, as
  the server reads them. Queries are prepared, so values never become SQL text.
- Values convert both ways: numbers, text, booleans, `json`/`jsonb` as maps and lists,
  arrays as lists (up to six dimensions), `numeric` as [decimals](#decimals) (an int
  when it has no scale), `money` as a decimal (with its `lc_monetary`'s decimals), `xid`
  and `xid8` as ints, `bit(n)` and `varbit` as their digits (`"00000101"`, kept at their
  length), and `inet`, `cidr`, `macaddr`, `uuid`, `date`, `time`, `timetz`,
  `timestamptz` (ISO 8601, in UTC, with `Z`), `timestamp` (as stored, with no zone),
  `interval` (PostgreSQL's own text, `1 mon 2 days 03:00:00`) and `pg_lsn` as strings,
  as PostgreSQL writes them. A BC date is ISO 8601's, `-0043-03-15` for 44 BC (`0000` is
  1 BC), and is written back as the same date. A `real` reads as the shortest float that
  prints the same (`0.1`, not `0.10000000149011612`). A type with no reader of its own
  (ranges, `hstore`, composites, `tsvector`, `regtype`) is its bytes in hex (`\x...`),
  the same way in every row: cast it in the query (`r::text`). An array whose lower
  bound is not 1 (`'[0:2]={1,2,3}'`) comes back as its text, which written back keeps
  its bounds, so a column's value may be a list in one row and text in another.
- JSON is read cell by cell by Quillo's own reader: a whole number past an int is a
  [decimal](#decimals), past a decimal a float, so two large ids stay two; a value that
  cannot be read (a number past a float, a nesting deeper than 128 levels) is an error value
  in its own cell (`db: this JSON could not be read: ...`), and the other cells and rows
  read; a lone surrogate is U+FFFD. A whole decimal inside a value sent is written as a
  JSON number, so it goes back as it came.
- Parameters are typed by the column: a `bool` takes only a boolean (not 1), the integer
  types take ints (or whole numbers and numeric text, range-checked: an int past the
  column's range is the server's own error, and compared with the column it matches no row), and other types are sent as text and cast.
  A list sent to an array column is an array, and a list inside a `jsonb[]`'s list is one
  JSON value. A text given as a `json` or `jsonb` column's value (an insert's values,
  `set doc = ?`) is the JSON it holds, as MySQL's JSON column takes it:
  `json.stringify(m)` stores an object, not a string; given to a function, it is a JSON
  string, as MySQL's `json_set` takes it. A float past `real`'s or `double precision`'s
  range is refused (`1e39 is out of range for type real`), where it was stored as
  Infinity or 0. A value where nothing in the SQL says its type (`select greatest(?,
  ?)`, `select ? as v`) is declared `int4`, `int8` or `float8` by what it is (text as
  text), so `greatest(10, 9)` is `10` and `select ? as v` with 5 is `5`, as on the others; when PostgreSQL refuses those types, only
  the values that refusal is about (a function's arguments, an operator's two sides) go as
  text, so `select upper(?) as u, ? as v` with 12345 and 5 gives `v` as `5`, as SQLite and
  MariaDB do; a statement is tried at most six times.
- `last_id` is `null`: use `insert ... returning id` with `db.one`.
- After a schema statement (`alter table`, `create or replace view`, a `drop`), the
  statements kept on what it names are prepared again, on every connection; one whose
  plan another server changed is prepared again and tried once.
- `?schema=` in the URL is added to `?options=`, not in its place.
- Connections come from a pool (8 by default; `?pool_size=N` in the URL) for
  transactions and migrations. A query releases the interpreter lock while it waits,
  so other requests keep running. A transaction keeps one connection until it ends, so
  other requests never see its changes early.
- **The session is the transaction's** *(since 0.7)*. A pooled connection goes from one
  request to another, so what a statement changes beyond its transaction would reach the
  next visitor: `set` (without `local`), `set_config(..., false)`, a `create temp` table,
  `listen`, a session's advisory lock, a cursor `with hold`, `set session authorization`.
  Every transaction's connection (`db.transaction`, `db.atomic` and migrations alike) is
  reset as it ends, before it goes back (`set session authorization default`, `reset
  all`, `close all`, `discard temp`, advisory locks let go, `unlisten *`; Quillo's
  prepared statements kept), or closed if the reset fails. Outside one, where the
  connection is shared by every request, such a statement is refused, in any form and
  place: `set_config` with values, inside `values (...)` or an `update`, and whatever its
  case or quoting, unless its third argument is `true`; a `pg_temp` table; and a `begin`
  anywhere in a script. A `do` block, and a call of a function of the database's own
  (which may change the session where Quillo cannot read it), runs outside a transaction
  on a connection of its own, reset after it. `prepare` is refused everywhere: pass values as
  parameters (`?`), which Quillo prepares itself. Inside a SQLite transaction a PostgreSQL
  query takes a free connection at once or is an error, as waiting would hold every request. Row-level
  security's setting per request is therefore written inside the transaction, as local:

  ```quillo
  db.transaction(() => {
      db.one("select set_config('app.user_id', ?, true)", me().id)   // true: this transaction's
      db.query("select * from orders")                                 // what its policies allow
  })
  ```

  A policy that reads it can take `coalesce(nullif(current_setting('app.user_id', true), ''), ...)`:
  after a `set local`, PostgreSQL reads the setting back as `""` in the connection's next
  transaction, not null.
- In the server, other queries share a second set of connections, pipelined (four,
  more while some wait on slow queries, up to `pool_size`): a request's queries (its
  `parallel` calls' too, outside a transaction) go to the same one, so the database
  reads them together. Size `max_connections` for twice `pool_size` per server and
  instance, and a few more for Quillo's own tables. Idle connections stay open; a
  connection that drops is opened again on the next query, and the query that met the
  drop fails.
- Prepared statements are kept, 256 per connection. There is no `COPY`, `LISTEN` or
  `NOTIFY`: bulk inserts are many-row `values (?, ?), (?, ?)` or `unnest(?::int[],
  ?::text[])` with lists. A Unix socket is `?host=/var/run/postgresql`. A query's time
  limit, or a read-only session, is the server's: `?options=-c%20statement_timeout%3D5000`
  in the URL, or `alter role`.
- Special characters in the user or password are percent-encoded in the URL (`%40` for
  `@`).
- TLS is required for a server on another machine, with its certificate and name
  checked; on this machine (`localhost`, `127.0.0.1`, `::1`, a Unix socket) it is used
  when the server offers it. The URL can say otherwise, as libpq's do: see
  [TLS to the database](#tls-to-the-database) below. Before, TLS was only preferred
  everywhere: a URL to another machine whose server has no TLS now needs
  `?sslmode=disable` to connect.

**MySQL and MariaDB** (the `mysql` feature, built in by default; `cargo build
--no-default-features` leaves it out):

- Placeholders are `?`, or `:name` with a map (translated, skipping quotes, backticks
  and comments: `#` lines, and `/*! */` and MariaDB's `/*M! */`, which the server runs,
  read as SQL). Statements are prepared, so values never become SQL text.
- Values convert both ways: numbers, text, `tinyint(1)` as booleans when it holds 0 or 1
  (another value is an int), `decimal` as [decimals](#decimals), MySQL's `json` as maps
  and lists (MariaDB's JSON is text: `json.parse` it), `BIT(n)` as its digits
  (`"00000101"`, as PostgreSQL's `bit`), which it takes back, and dates and times as
  text (`2026-10-27 14:30:00`, with a space and no zone; `TIME` as `HH:MM:SS`, hours past
  24 too). A `FLOAT` reads as the shortest float that prints the same, and a value a
  `FLOAT` column would keep as 0 (`1e-46`) is refused as out of its range. An unsigned
  value or an id past 2^63 is a [decimal](#decimals) (`9223372036854775808m`), which finds
  its own row. `last_id` is the new row's `auto_increment` id (see above for several
  rows).
- `db.open` of a database that is not there is an error (`Unknown database 'shop'`), as
  on PostgreSQL, so a typo in a URL makes no empty database; a tenant's is made by the
  driver (below). A host may be an IPv6 address, `mysql://app@[::1]:3306/app`.
- A statement larger than the server's `max_allowed_packet`, its values included, is
  refused, saying so and how big it is, where the connection broke.
- Inside `db.transaction` and `db.exec_many`, a statement that commits by itself
  (`truncate`, `create table`, `alter`, `drop`, ...) is refused before it runs, so what
  ran before it can still be rolled back: `` `db.exec("truncate table rates")` inside a
  transaction: MySQL and MariaDB commit the transaction around this statement by itself
  ... run it before or after the transaction ``. One that cannot be seen beforehand (a
  procedure's, `execute immediate`, a `begin not atomic` block) ends the transaction with
  the same error, read from the server's in-transaction flag. PostgreSQL and SQLite roll
  their schema statements back, and take them in a transaction.
- Connections come from the driver's pool (10 to 100), and a query releases the
  interpreter lock while it waits, as with Postgres. `pool_size` is PostgreSQL's: in a
  MySQL URL it stops the database from opening; the driver's own `pool_min` and
  `pool_max` set the pool. A transaction keeps one connection until it ends; nested
  ones are savepoints. A Unix socket is `?socket=/path`.
- `db.migrate()` takes turns through a named lock (`get_lock`). A migration may change
  the schema, but such a statement commits at once in MySQL (outside the refusal above),
  so a migration that fails part way keeps what it did before the failure: put one
  change in each file.
- A database per tenant: `mysql://host/app_{tenant}`, made when the program makes the
  tenant, and listed by `db.tenants()`.
- TLS is required for a server on another machine, with its certificate and name
  checked; on this machine (`localhost`, `127.0.0.1`, `::1`, `?socket=...`) it is not
  used unless the URL asks for it. See [TLS to the database](#tls-to-the-database)
  below. Before, TLS was off unless `require_ssl=true`.

### TLS to the database

Some hosted databases sign their certificates with a certificate authority (CA) of
their own: Amazon RDS and Aurora, Google Cloud SQL (the instance's `server-ca.pem`), and
a company's servers with its private CA. The public CAs that are trusted by default do not know those, so the
connection fails with `invalid peer certificate: UnknownIssuer` until the URL names
the CA's certificates (a PEM file, as the service publishes it):

```quillo
// Amazon RDS and Aurora: their CAs' bundle, saved where the program runs, from
// https://truststore.pki.rds.amazonaws.com/global/global-bundle.pem
db.open("postgres://app@mydb.abc123.eu-west-1.rds.amazonaws.com:5432/app?sslrootcert=/srv/app/global-bundle.pem")
db.open("mysql://app@mydb.abc123.eu-west-1.rds.amazonaws.com:3306/app?ssl_ca=/srv/app/global-bundle.pem")
// Or in $DATABASE_URL, with the password: postgres://app:...@...rds.amazonaws.com/app?sslrootcert=...
```

PostgreSQL reads the options libpq does:

| In the URL | TLS | The server's certificate |
|---|---|---|
| `sslmode=verify-full` (the default for another machine) | always | signed by a trusted CA, for the host's name |
| `sslmode=verify-ca` | always | signed by a CA in `sslrootcert`, for any name |
| `sslmode=require` | always | not checked, unless `sslrootcert` is given (then as `verify-ca`) |
| `sslmode=prefer` (the default on this machine) | when the server offers it | as with `require` |
| `sslmode=disable` | never | |

- `sslrootcert=/path/ca.pem`: the CAs to trust, in PEM, instead of the public ones.
  `sslrootcert=system` trusts the operating system's CAs (with `verify-full` only).
- `sslcert=/path/client.crt` and `sslkey=/path/client.key`: a certificate and its key
  (PEM), for a server that asks for one (`hostssl ... cert` in `pg_hba.conf`).
- `require` keeps the connection from being read, but not someone in the middle from
  posing as the server, who then has the queries and the data. Use it only where the
  network is yours; `verify-ca` with the CA's certificate costs no more. `prefer` is
  weaker still: someone in the middle can say the server has no TLS.
- `verify-ca` without `sslrootcert` is refused: any certificate from a public CA, for
  anyone's site, would pass. Where the server is reached at an address its certificate
  does not name, `verify-full` with the name in `host` and the address in `hostaddr`
  (`postgres://db.internal/app?hostaddr=10.0.0.5`) checks the name too.

MySQL and MariaDB take the MySQL driver's options:

- `ssl_ca=/path/ca.pem`: the CAs to trust instead of the public ones (with
  `built_in_roots=true`, as well as them). It asks for TLS on this machine too.
- `ssl_server_name=db.example.com`: the name the certificate is checked for, where the
  host connected to is an address or a tunnel. `verify_identity=false` alone is
  refused, since the driver checks the name regardless; with `verify_ca=false` too,
  nothing is checked.
- `verify_ca=false`: the certificate is not checked at all, which has the risk of
  Postgres's `require` above.
- `ssl_cert=/path/client.crt` and `ssl_key=/path/client.key`: a client certificate
  (`REQUIRE X509`).
- `require_ssl=true` asks for TLS on this machine; `require_ssl=false` turns it off.

## Several databases {#standard-library-several-databases}

`db` is the program's database. `db.connect(target)` opens another, with the same
functions, and its own transactions, migrations and live updates:

```quillo
db.open("app.db")
let analytics = db.connect(env("ANALYTICS_URL"))           // any kind: SQLite, Postgres, MySQL
analytics.migrate("migrations/analytics")
analytics.query("select day, sum(total) from sales group by day")
db.transaction(() => {
    db.exec("insert into orders ...")
    analytics.transaction(() => analytics.exec("insert into sales ..."))   // its own
})
```

- Call its functions on it, `analytics.query(...)` (or `sql | analytics.one`): that is
  how they know which database. A connection has `exec`, `query`, `one`, `transaction`,
  `migrate`, `tenants`, `tenant_exists` and `sql_name`; `error_kind`, `connect`, `system`, `shards`
  and `atomic` are `db`'s. Connect at the top of the program; the same target
  again is the same connection, not a new pool. A SQLite file is the same file by any
  spelling: `db.connect("./analytics.db")`, `"sub/../analytics.db"` and the full path
  are one connection.
- Its calls need the `db` effect, as `db`'s do (see [Effects](#effects)).
- A transaction is its own database's: one inside another on a different database
  commits or rolls back on its own.
- Live data tells them apart: a page showing `analytics`' `orders` does not refresh
  when `db`'s `orders` changes. Its topics carry a label, not the URL: a SQLite file's
  real path (from the program's folder when it is inside it), or a server's scheme,
  host, port and database, without the user, the password or the options. So two
  servers on one database agree on its topics, during a password's rotation too, and a
  page reading through one connection hears a write made through another. (Another
  user or other options on a server URL is another pool, with the same topics.)
- **Names for this database.** `analytics.sql_name(col)` quotes a name as that database
  reads it, `` `col` `` on MySQL, `"col"` elsewhere; `db.sql_name(col)` *(since 0.7)* does the same for
  the program's database. Plain `sql_name` quotes for the program's database only, so a
  program with MySQL beside SQLite or PostgreSQL uses the connection's: MySQL reads
  `"title"` as text, and `select "title" from items` gives the word `title` in every row.
  None of them needs an effect, and `quillo check` takes SQL built with them as safe.

```quillo
db.open("app.db")
let analytics = db.connect("analytics.db")
print(analytics.sql_name("total"), db.sql_name(["day", "total"]))
print(db.connect("./analytics.db") == analytics)
```

```
"total" "day", "total"
true
```
- A connection never reaches the browser (its URL may hold a password).

Mounted programs and services have their own `db` too, so one Quillo can run programs
on different databases side by side.

**Read replicas.** A PostgreSQL or MySQL database's replicas take its reads:

```quillo
db.open(env("DATABASE_URL"), { replicas: [env("REPLICA_1"), env("REPLICA_2")] })
```

- Reads (`select`, and `with` that writes nothing) go to the replicas in turn. Writes,
  reads that lock (`for update`, `for share`), anything in a transaction, and every
  read of a request after it wrote go to the primary: a request reads what it wrote.
  Only pure reads go to a replica: a `select` that calls a function that may write
  (`nextval`, `setval`, or one made in the database with `create function`, on
  PostgreSQL and MySQL alike) goes to the primary; built-in functions that only compute
  (`lower`) do not. A replica's read-only refusal sends that statement to the primary
  without taking the replica out of use.
- Every replica must open when the database does, or `db.open` fails. A replica that
  fails later is left out for ten seconds; its read goes to the primary. Replicas are
  for a database named by its URL: not SQLite, not a database per tenant.
- Another request may still read from a replica that has not caught up with a write
  made just before. Reads that cannot wait for that belong in a transaction.
- `db.connect(url, { replicas: [...] })` too.

**Shards.** Data split over several databases by a key (a customer, an account):

```quillo
let users = db.shards([env("SHARD_1"), env("SHARD_2"), env("SHARD_3")])
users.migrate()                                           // each one
users.on(id).exec("insert into users (id, name) values (?, ?)", id, name)
users.on(id).one("select * from users where id = ?", id)   // the shard for the key
users.all("select count(*) as n from users")               // every shard's rows, together
```

- `.on(key)` is a connection, as `db.connect` gives (its own transactions too). Keys
  are spread by jump consistent hashing of their text (so `1` and `"1"` are one key),
  the same on every server: a shard added at the end takes its share of keys from each
  of the others and moves no other key (those are the rows to copy over).
- `.all(sql, ...)` asks the shards one after another, and stops at the first that
  fails; an aggregate comes back once per shard. `.count` is how many there are.
  `query`, `one`, `exec` and `transaction` on the shards themselves are an error.
- Shards may be any kind of database, and mixed.

**Joining across databases.** One SQL query cannot join tables of two databases;
`join_on` puts their rows together in the program:

```quillo
let orders = db.query("select * from orders where day = ?", today)
let people = accounts.query("select * from users where id = any(?)", orders | map((o) => o.user_id))
join_on(orders, people, "user_id", "id")      // each order with `user`: its user, or null
```

`join_on(rows, others, key, other_key = key, as)` gives each row its first match (same
value, same type) under `as`, which is `key` without `_id` (`user`), else `joined`;
`| filter((o) => o.user != null)` keeps those with one.

**One transaction over several databases.** `db.atomic` keeps all of it, or none:

```quillo
db.atomic([db, ledger], () => {
    db.exec("update accounts set balance = balance - ? where id = ?", n, from)
    ledger.exec("insert into entries (account, amount) values (?, ?)", from, -n)
})
```

- The list names the databases that take part: `db`, connections from `db.connect`, a
  shard (`users.on(id)`), or shards whole (`users`: each of them). A database the
  function uses that is not in the list is not part of it: its writes stay when the
  rest rolls back.
- It commits in two phases: each PostgreSQL and MySQL database prepares its part
  (prepared transactions, XA), the decision to commit is recorded with Quillo's own
  tables, a SQLite database (one at most) commits, and the prepared parts commit. A part
  that cannot prepare (a deferred constraint, a database gone) rolls every part back,
  as does a function that fails, returns an error value or replies early.
- A server that stops part way leaves prepared parts, which hold their locks:
  recovery finishes them a minute or so later (it looks every 30 s for parts over a
  minute old, from any server of the program), committing those decided and rolling
  back the others. The first to record a decision for a transaction, it or recovery,
  decides it. Recovery looks at the program's database and its connections, not at a
  database per tenant. At most one SQLite database may take part (naming two is a
  runtime error).
- PostgreSQL needs `max_prepared_transactions` set above 0 (it is 0 by default).
- `db.transaction` inside it nests as usual; `db.atomic` cannot begin inside a
  transaction on one of its databases.

## Quillo's own tables {#standard-library-quillos-own-tables}

Scheduled jobs (`_quillo_jobs`: each run once, whichever server it is due on) and the
decisions of `db.atomic` (`_quillo_atomic`) are kept in the program's database. When
there is a database per tenant they cannot be, and go to the system database instead:
`db.system(target)` (a database every server of the program shares, for several servers),
`$QUILLO_SYSTEM_DB`, or else a SQLite file `quillo-system.db` next to the program.
`db.system` also keeps them apart from the program's own tables when there is one
database.

What must commit with the work is kept with the work, in the program's database, or the
tenant's when there is one per tenant, whatever `db.system` says: the answers of
[idempotent requests](#idempotent-requests) (`_quillo_idempotency`) and calls made later
(`_quillo_later`, with the answers of those finished in `_quillo_later_done`). A call made
with no database to go with goes to the system database, so that it outlives a restart;
a program with no database at all has `quillo-system.db` for that (its jobs need no
record: one server runs them). With a database per tenant, the system database notes
which tenants have calls (`_quillo_later_tenants`), as nothing else says where to look.

The migrations applied (`_quillo_migrations`) are in the program's database. The calls a
service answered (`_quillo_calls`) go with the jobs. `quillo-system.db` is made only when
something must be kept in it (calls made later, `db.atomic`'s decisions);
`$QUILLO_SYSTEM_DB` may be a URL.

## Many tenants

One program can serve many tenants (customers, teams, sites), each with its own data:

```quillo
tenant_from("example.com")          // acme.example.com is tenant "acme"
db.open("data/{tenant}.db")         // a SQLite file per tenant
db.migrate()                        // run for each tenant's database when it is made

route GET /signup =                 // at www.example.com, which needs no database
    <form method="post" action="/signup">
        <input name="name" pattern="[a-z0-9][a-z0-9_-]*" placeholder="acme" required/>.example.com
        <button>Make my site</button>
    </form>

route POST /signup { form: { name: str } } with db {
    let made = new_tenant(name, () => db.exec("insert into settings (plan) values ('free')"))   // makes acme.db
    if is_err(made) { reply 409 "That name is taken" }
    redirect(`https://${name}.example.com/`)
}
route GET / = <h1>Welcome, {tenant()}</h1>
```

Sent `acme`, the form makes `data/acme.db` (with `migrations/`'s tables) and goes to
`https://acme.example.com/`, which says `Welcome, acme`. Sent `acme` again, it shows
`That name is taken`, with status 409.

- `tenant_from("example.com")` takes the part of the host before the domain;
  `tenant_from((request) => ...)` takes what the function gives for the request (a
  header, a path segment, a lookup), a name or null. `tenant()` is the current one, or
  null. A tenant's name is lowercase letters, digits, `-` and `_` (at most 63), starting
  with a letter or a digit; anything else is no tenant, so a name is always safe in a
  file name or a database name.
- A function as the target says where each tenant's database is, so tenants can be on
  different kinds of database. It is asked once for each tenant, when first used; null
  means there is no such tenant. It cannot use the database it picks for (keep the list
  in another):

  ```quillo
  let registry = db.connect("registry.db")
  db.open((tenant) => registry.one("select url from tenants where name = ?", tenant)?.url)
  ```
- A `db.open` target with `{tenant}` in it is a database per tenant: a SQLite file each,
  a PostgreSQL schema each in one database with `?schema=t_{tenant}`, or a database each
  with `postgres://host/app_{tenant}` (made by you beforehand), or a MySQL database each
  with `mysql://host/app_{tenant}` (made by Quillo). Give schemas a prefix:
  `public`, `information_schema` and `pg_...` are refused, and a prefix keeps a tenant
  named like another schema of the database (`auth`, say) out of it. A tenant's
  connections search its schema alone, so what is in `public` (an extension's functions)
  is named with it: `public.gen_random_uuid()`. `?schema=app` without `{tenant}` keeps
  one program's tables in a schema of its own.
- `db.tenants()` lists the SQLite files, the PostgreSQL schemas or the MySQL databases
  there are; for `postgres://host/app_{tenant}`, and for a function, it is an error:
  keep the list yourself.
- `new_tenant(name, f)` *(since 0.7)* is for signing up: it makes the tenant's database, migrates it and
  runs `f` for it, or gives an error when that tenant exists, so a second signup for one
  name cannot land in a customer's data. It is one step on the database, so of two signups
  at once one makes it. Names such as `www` and `admin` are the program's to refuse.
- A request's host can name any tenant, so requests only open databases that exist, and
  make no file or schema. A request for a made-up `anything.example.com` that needs its
  database answers 404, unless the route answers for itself; one that needs no database
  answers as usual, so `www.example.com` can be a page of its own.
- Each open tenant has its own connections (a pool and its shared ones, as above), so
  give PostgreSQL a small `pool_size`. At most 1,024 SQLite tenants' databases, and 16
  PostgreSQL or MySQL ones, are open at once, the least recently used closed first;
  others are opened again when needed, their migrations run on each opening, and past
  the limit each request connects again: 3 ms against 0.2 ms on one machine, more over
  TLS. `?open_tenants=64` in the URL keeps that many open, at `pool_size` connections
  each: leave room in the server's `max_connections`. Connections are never shared
  between tenants, as they keep statements prepared against one schema. Code that runs
  for no tenant, such as a request to the bare domain, gets an error from `db`.
- Live updates stay within a tenant: a change to one tenant's table wakes only that
  tenant's pages, even when the tenants share one database, and a write made for no
  tenant (a job, a script) wakes none. A page follows only its own tenant's topics and
  those of no tenant: one naming `acme@db:orders` from evil's host is not told of acme's
  orders *(since 0.7)*. Topics are not checked further: within a tenant, any page may
  follow any topic it can name.
- Channels and media streams are each tenant's own, unless `tenants: "shared"` (see
  [Channels](#channels) and [Media](#media)).
- `with_tenant("acme", () => ...)` runs code for a tenant, making its database if there is
  none: outside a request, in a script or a scheduled job.
- What stays shared: global variables, one copy for the whole process, so keep a
  tenant's data in its database (or in maps keyed by `tenant()`). `quillo check` warns on a
  function or route that writes a global in a program with `tenant_from`. Views that call
  `tenant()` or read the database are never served from the view cache. Cookies
  belong to the host they were set on, so each subdomain has its own.

## HTTP client

```quillo
let r = http.get("https://api.example.com/users/1")        // { status, ok, headers, body, json }
http.post(url, { name: "Ada" })                            // maps and lists are sent as JSON
http.request("PUT", url, body, { authorization: `Bearer ${token}` })
```

Every HTTP status is a response; failing to connect is an error value, and so is a URL
that is not `http://` or `https://`. The functions: `http.get(url, headers, options)`,
`http.delete(url, headers, options)`, `http.post(url, body, headers, options)`,
`http.put` and `http.patch` as `post`, and `http.request(method, url, body, headers,
options)`. The answer is `{ status, ok, headers, body, json, cookies }`: `ok` for a 2xx,
header names in lower case, a header sent more than once joined with `, ` (two `Link`s are
one `</x>; rel=a, </y>; rel=b`), `set-cookie`'s lines joined by a line break, as they
cannot be by a comma, and also given one by one in `cookies`, a list; `body` as text
(bytes that are not UTF-8 replaced), `json` the body parsed, or `null`. A body longer than
64 MiB is an error value that says to read it with `http.stream` *(since 0.7: it was cut
there, and cut JSON was `null`)*.
Interim answers (`100 Continue`, `103 Early Hints`) are passed over to the final one. A
body in `gzip`, `deflate` or `br` is decoded; another coding is an error value that names
it (`the answer is in a coding Quillo cannot read: content-encoding zstd`). Requests say `User-Agent: quillo/<version>` unless
the headers give another; some APIs (OpenStreetMap's Nominatim) refuse that one, so
name your program. `http.stream(url, f, opts)` reads an answer as it arrives (see [Streaming](#streaming)).
Text is sent as `text/plain`, HTML as `text/html`, other values (numbers and decimals
too) as JSON, unless the headers give a `content-type`.

Options come after the headers (in `http.stream`, in its map):

```quillo
http.get(url, {}, { redirects: 0 })                 // a redirect is the answer, not followed
http.post(url, body, {}, { public_only: true })     // only addresses on the public internet
http.get(`https://api.example.com/rates`, { authorization: key }, { cache: 60 })
http.get(url, {}, { timeout: 5 })                   // an error value after 5 seconds
```

- `redirects`: how many to follow, from 0 to 20 (5 unless given). A 301, 302 or 303 after
  a `POST` is followed as a `GET` without the body, as browsers do; a 307 or 308 after a
  write is followed with the same method and body. `Authorization` goes on to the same
  host, and `Authorization` and `Cookie` are not sent on to another origin.
- `public_only: true` connects only to the public internet: not this machine, a private
  network, or a cloud's metadata service (`169.254.169.254`), however the address is
  written, checked on the address the name gives as it connects (and again for each
  redirect). Use it whenever someone else may choose the URL. `quillo check` warns about
  a call whose host comes from a value (a parameter, `request`, text put together before
  the host ends) unless its options say `public_only`, either way: `false` says the call
  is meant to reach inside.
- Under a policy (see [The policy](#the-policy)), `[http] hosts` lists the hosts any
  call may reach, and `private = false` keeps every call off this machine and its
  networks, as `public_only` does, whatever the call's options. Each redirect is judged
  too, before it is followed. A call refused is an error value: `http: example.com is not
  among the hosts quillo.policy allows: api.stripe.com, *.mapbox.com`.
- `cache: seconds` (a GET's, up to a day) keeps a successful answer and gives it again to
  the same call, the same URL, headers and options, from any request, until then. Calls
  to a site that limits how often it is asked, or that charges for each, then go out once
  for everyone: `cache: 60` on the rates above asks once a minute however many pages
  show them. Failures are not kept, nor answers over 16 MiB. At most 64 MiB of answers
  are, those that end soonest going first. On a method other than GET, and in
  `http.stream`, `cache` is an error.
- `timeout: seconds` (more than 0, up to 300; `0.5` too) is how long the whole call may
  take: then it gives an error value (`... took more than 5 s (its timeout)`). Unless given,
  a call has 10 seconds to connect and 60 in all; `http.stream`, 300 seconds of silence
  between pieces. A page that waits on a slow service waits that long.

- `ca`: the certificate authorities to trust instead of the usual public ones, a PEM file
  (named from the program's folder) or `"system"`, the machine's own store: for an
  internal service under a company's CA.
- `cert` and `key`: a client certificate and its key, PEM files, presented to the server,
  as some banks' and governments' APIs ask (mutual TLS). One without the other is an error
  (`` `cert` and `key` go together ``), and so is a file with no certificate in it.

```quillo
http.get("https://ledger.internal/balance", {}, { ca: "certs/internal-ca.pem" })
http.post("https://api.bank.example/payments", order, {}, { cert: "certs/client.pem", key: "certs/client.key" })
```

Another option is an error, in `quillo check` when the options are written in the call,
and when it runs: `{ timout: 5 }` is not ignored.

Calls go through the proxy `HTTPS_PROXY` names for `https://` and `HTTP_PROXY` for
`http://` (in either case, `ALL_PROXY` for both), unless `NO_PROXY` names the host (a
name, `.example.com` or `example.com` for it and those under it, an address, or `*`).
This machine (`localhost`, a name under `.localhost`, a loopback address) is never reached
through one. With a proxy, the proxy looks the name up, so `public_only` checks the host
before the call is made.

In a component, the same calls go from the page (see [What the browser can reach](#what-the-browser-can-reach)):
`redirects: 0` makes a redirect an error value there, `timeout` is the same, and
`public_only` and `cache` are the server's. A service that needs the visitor's address, called from the server, can be
given `request.ip`.
