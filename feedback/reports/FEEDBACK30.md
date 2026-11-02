# Feedback from building a clinic's booking desk across five time zones in Quillo

| | |
|---|---|
| App | [app/clinic/](../../app/clinic): Quillo Clinic, one booking desk for five branches in five zones (Manila; New York's all-night urgent care; Sydney; London; Tokyo). Patients book a doctor's free time in the branch's own time and see it in theirs. Invoices come in the branch's currency with its tax, lab results are uploaded by patients and by the front desk, invoices export as CSV, and a reminder is written a day ahead in the patient's language. English, Spanish and Arabic (right to left). 692 lines of Quillo in 11 files, 857 lines with the translations, schema and CSS |
| Quillo | 0.6.0 (6ec94b4, 2026-11-01), built from `git archive 6ec94b4` in a scratch folder, as the checkout holds another session's uncommitted `feedback29-fixes` |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check`; headless Chrome (`feedback/tools/`), with the visitor's zone set to Berlin and Singapore; `mismatches.mjs` on every page as three users; curl for multipart bodies, cookies and server fn calls; 2,000 appointments for one patient; 64 date, number and money cases computed by the server and by a component and compared |

What worked well: time zones. `date.at` gets every daylight-saving change right on both sides, including Lord Howe Island's half-hour shift and the day Samoa skipped in 2011. A doctor's day comes out right in New York on both days the clocks change. The server and the browser agreed on 61 of 64 cases of dates, money and numbers. The other three are documented: a builtin's error text, hidden on the server, and a whole float written `1.0` on the server outside a component. Booking, the race for one time (the unique index lets one win, and the other is told in their own language), invoices with 8.875% tax in dollars and 10% in yen, the reminder job, Arabic's six plural forms and `dir="rtl"` all worked on the first or second try. A page of 2,000 appointments resumes with nothing remade, and cancelling one takes 31 ms.

Fix Q-361 first: one careless upload route lets a visitor replace the running program. Q-362 to Q-364 are next. Each fails silently for whole groups of visitors: French visitors served English, Taiwan and Hong Kong served Simplified Chinese, and files that never arrive.

The repros below are inline, each run on the version above. No Quillo source was changed, and neither `feedback/repros/` nor `feedback/INDEX.md` was touched, as another session is working in the checkout (as for FEEDBACK29). Numbers start at Q-361, after FEEDBACK29's Q-360.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-347 | `with_locale` in a helper that a component's click also called: `quillo check` said ok, and `quillo run` refused to start (`` `with_locale` is not available in the browser ``). The guide says check looks only at what a component calls directly | 1 run |
| Q-359 | `db.exec(sql, ...row)` for a seed row: `` expected an expression, found `..` `` | 1 check |
| Q-200 | Its fix covers form fields, and Q-365 is the same problem for files in a multipart body | |

## New issues

### Q-361. `save_upload` writes over the program's own files, and under `--reload` an upload runs as the program

| Kind | Detected by | Attempts |
|---|---|---|
| silent, security | probing what a careless route allows | 1 |

The guide says to name saved files yourself, and `save_upload` does guard two things: it refuses `..`, and it drops the folders from `filename` (`C:\fakepath\report.pdf` arrives as `report.pdf`). But it writes anywhere in the program's folder, the program's own files included. A route that saves under the visitor's name at the folder's root lets anyone replace `main.qwe` or `.env`. Under `--reload`, the production mode, the new `main.qwe` is loaded within a second and runs:

```quillo
route POST /drop {
    let f = request.files.doc
    save_upload(f, f.filename) ?? "STOP"
}
route GET / = "clinic"
```

```sh
echo 'API_KEY=secret-key' > .env
quillo run main.qwe --port 18308 --reload
printf 'route GET / = `attacker sees ${env("API_KEY")}`\n' > /tmp/evil.qwe
curl 127.0.0.1:18308/                                                    # clinic
curl -F 'doc=@/tmp/evil.qwe;filename="main.qwe"' 127.0.0.1:18308/drop   # main.qwe
curl 127.0.0.1:18308/                                                    # attacker sees secret-key
```

The log says only `quillo: reloaded main.qwe (1 routes)`. An upload named `.env` replaces that file the same way, to be read at the next start, and `quillo check` passes the route. A path built from a form field reaches the root too: `` `${request.form.folder}/${f.filename}` `` with the folder `.` writes `./main.qwe` (an empty folder gives `/main.qwe`, which is refused).

**Suggestion:** have `save_upload` and `write_file` refuse the program's own files: the `.qwe` and `.qwer` files it loaded, `.env`, the open SQLite file with its `-wal` and `-shm`, and `migrations/`. Better still, take uploads only under a folder the program names. Have `quillo check` warn when the path is `f.filename` or ends in it with no folder before it.

### Q-362. A locale file named `fr_FR.json` is loaded, and never chosen

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe of locale names | 1 |

`translations` accepts `_` in a locale's name, as gettext and Java name their files. When it matches a request, it turns `_` into `-` in the tag asked for, but compares that with the file's name as written. So `fr_FR` never matches anything: not `Accept-Language: fr-FR`, not `fr`, and not even a `locale=fr_FR` cookie. Every visitor gets the default, with no word at load or at request:

```sh
mkdir locales
echo '{ "hi": "Hello" }'   > locales/en_US.json
echo '{ "hi": "Bonjour" }' > locales/fr_FR.json
echo 'translations("locales")
route GET / = `${locale()} ${t("hi")}`' > main.qwe
quillo run main.qwe --port 18301
curl -H 'Accept-Language: fr-FR' 127.0.0.1:18301/    # en_US Hello
curl -H 'Accept-Language: fr' 127.0.0.1:18301/       # en_US Hello
curl -b 'locale=fr_FR' 127.0.0.1:18301/              # en_US Hello
```

**Suggestion:** make locale names one form as they load (`fr_FR` becomes `fr-FR`, as `best` does to what is asked), or refuse `_` with a message that names the file.

### Q-363. Taiwan and Hong Kong get Simplified Chinese, and `sr-Latn` gets Cyrillic

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe of script subtags | 1 |

With `zh-Hans.json` and `zh-Hant.json`, the guide's own example names, a request is matched exactly, then by language alone. So every `zh-*` the files do not name exactly takes the first `zh` file by name, which is `zh-Hans`. That includes a tag that names its script:

| `Accept-Language` | Served | Should be |
|---|---|---|
| `zh-TW`, `zh-TW,zh;q=0.9` (what a Taiwanese browser sends) | zh-Hans | zh-Hant |
| `zh-Hant-TW` | zh-Hans | zh-Hant |
| `zh-HK` | zh-Hans | zh-Hant |
| `sr-Latn-RS` (with `sr-Cyrl.json` and `sr-Latn.json`) | sr-Cyrl | sr-Latn |
| `zh-CN`, `zh-Hans-CN` | zh-Hans | zh-Hans |
| `pt-AO` (with `pt-BR`, `pt-PT`) | pt-BR | pt-PT, by CLDR's parent locales; minor |

Repro: a `locales/` folder with those files, each `{ "hi": "..." }`, and the `main.qwe` of Q-362.

**Suggestion:** match a script subtag before the language, and infer the script for a region as CLDR's likely subtags do: `zh-TW`, `zh-HK` and `zh-MO` are Hant, `zh-CN` and `zh-SG` Hans.

### Q-364. A file given to a `server fn` inside a list or a map arrives as `{}`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

Only a file passed as an argument on its own goes up as a part. One inside a list or a map is sent as JSON, and a browser `File` turns into `{}`. So the function gets maps with no `filename` and no bytes, and `save_upload` fails later on what looks like a good call. A whole `FileList` becomes a map of empty maps:

```quillo
server fn one(file: map) = file.filename
server fn many(files: list) = files | map((f) => f.filename)
server fn anyv(x) = x
server fn in_map(m: map) = m.doc.filename
component Up() {
    state out = []
    <div>
        <input type="file" multiple @change={(e) => {
            let fl = e.target.files
            out = [one(fl[0]), many([fl[0], fl[1]]), many(fl), anyv(fl), in_map({ doc: fl[0] })]
        }}/>
        <pre>{json.stringify(out)}</pre>
    </div>
}
route GET / = <html><body><Up/></body></html>
```

With two files picked: `one` gives `"xray.png"`, `many([fl[0], fl[1]])` gives `[null, null]`, `many(fl)` a 400 (`must be list, got map`), `anyv(fl)` `{"0": {}, "1": {}}`, and `in_map` null.

**Suggestion:** send every `File` found anywhere in the arguments as a part, and a `FileList` as a list of them. Or refuse in the browser with a message that says to pass each file as an argument of its own.

### Q-365. A `multiple` file field is a map with one file and a list with two

| Kind | Detected by | Attempts |
|---|---|---|
| silent, or a 500 | curl, while writing the desk's upload | 2 |

`request.files.docs` is the file's record when one file is sent, and a list when there are several, so code written for either breaks on the other. `for f in` over a single file walks the record's `[key, value]` pairs. The list pattern that fixed Q-200 for form fields refuses a single file:

```quillo
route POST /many { files: { docs: [map] } } = docs | map((d) => d.filename)
route POST /many = respond(400, "pattern did not match")
route POST /loop = for f in request.files.docs => f.filename
```

```sh
curl -F docs=@a.txt 127.0.0.1:18306/many                  # pattern did not match
curl -F docs=@a.txt -F docs=@b.png 127.0.0.1:18306/many   # ["a.txt","b.png"]
curl -F docs=@a.txt 127.0.0.1:18306/loop                  # 500: list has no field `filename`
```

The app works around it with `fn as_list(f) = f == null ? [] : type_of(f) == "list" ? f : [f]`.

**Suggestion:** let `[map]` in a `files` pattern take one file as a list of one, and none as an empty list, as `[str]` does for form fields. Say in Uploads that the shape follows the count.

### Q-366. Plural forms the language's rules never pick are taken without a word: English's `zero`, Romanian's `few`, all of Lithuanian

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the app's "No free times", then a probe | 1 |

A language without built-in rules counts as English (`one` for 1, else `other`), and a form its rules never pick is loaded and ignored. A translator who writes English's `zero` gets "0 messages", not "No messages". Rails' i18n and i18next both take a `zero` form for 0 in any language, so it is what people coming from them write. In Romanian, 0 and 2 to 19 take `few` ("2 zile") and 20 takes `other` ("20 de zile"), but Quillo writes "2 de zile". Lithuanian, Latvian, Slovenian, Icelandic, Macedonian, Irish, Welsh and Maltese all differ from English too:

```quillo
translations({
    en: { msgs: { zero: "No messages", one: "{count} message", other: "{count} messages" } },
    ro: { zile: { one: "{count} zi", few: "{count} zile", other: "{count} de zile" } },
    lt: { d: { one: "{count} diena", few: "{count} dienos", other: "{count} dienų" } },
})
print(t("msgs", { count: 0 }))                                                // 0 messages
print(with_locale("ro", () => [2, 19, 20] | map((n) => t("zile", { count: n }))))   // ["2 de zile", "19 de zile", "20 de zile"]
print(with_locale("lt", () => [2, 21] | map((n) => t("d", { count: n }))))          // ["2 dienų", "21 dienų"]: should be "2 dienos", "21 diena"
```

In the app, `"slots": { "zero": "No free times", ... }` showed "0 free times" in English, and "today" for 0 days ahead showed "in 0 days". French's `many`, for a million and more in CLDR 38 and later, is ignored too.

**Suggestion:** use a `zero` form, where one is written, for 0 in every language. Add CLDR's rules for the languages above (`ro` sits in `money.format`'s own list), or carry CLDR's plural table, which is small. At load, warn of a form the language never picks.

### Q-367. A count given as text, or as a decimal, picks `other`: "1 items"

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe | 1 |

`t` picks a plural form only when `count` is an int or a float. A count read from a query or a form is text, and `other` is taken for it. A decimal (a weight, an amount) does the same, on both sides:

```quillo
translations({ en: { items: { one: "{count} item", other: "{count} items" } } })
route GET / = t("items", { count: request.query.n })      // ?n=1 → "1 items"
print(t("items", { count: 1m }))                           // 1 items
```

`quillo check` says nothing.

**Suggestion:** take numeric text and decimals at their number, or make a `count` that is not a number a runtime error.

### Q-368. Date-times compare as text, and a fraction of a second breaks both equality and order

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a booking sent as JavaScript writes times | 1 |

The guide says date-times sort and compare in time order when they share an offset, as in UTC. That fails once one of them has a fraction of a second, and `toISOString()`, the way every JavaScript client writes a time, always writes one. `date.to_zone(t, "UTC")` keeps it, so the usual way to make times alike does not help:

```quillo
let a = "2027-03-15T15:00:00Z"
let b = "2027-03-15T15:00:00.000Z"
let c = "2027-03-15T15:00:00.500Z"
print([a == b, date.ms(a) == date.ms(b), date.to_zone(b, "UTC")])   // [false, true, "2027-03-15T15:00:00.000Z"]
print(sort([a, c]))      // ["2027-03-15T15:00:00.500Z", "2027-03-15T15:00:00Z"]: the later first
print(c < a)             // true
```

In the app, booking `2027-03-15T15:00:00.000Z`, a free slot, was refused as "closed", since the slot reads `...15:00:00Z`. The app compares instants as text, as the guide suggests, and keeping them in UTC does not save it.

**Suggestion:** give a way to make two times one text: `date.to_zone(t, "UTC")` could drop a fraction of zero, or a `date.utc(t)` could write one fixed form. Add `date.compare(a, b)`. Change the guide's sentence to name the fraction, and `date.ms` as the safe comparison.

### Q-369. `format_number` rounds half to even, though the guide says it writes as `Intl.NumberFormat` does; `round` rounds half up

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the server and browser comparison, then Chrome's own `Intl` | 1 |

The two sides agree with each other, but not with `Intl`, nor with `round` on the same float:

| | `format_number(x, "en-US", d)` | `round(x, d)` | Chrome's `Intl.NumberFormat` |
|---|---|---|---|
| 2.5, 0 | "2" | 3 | "3" |
| 0.125, 2 | "0.12" | 0.13 | "0.13" |
| 1.125, 2 | "1.12" | 1.13 | "1.13" |
| 1.005, 2 | "1.00" | 1.0 | "1.01" |
| 12.345, 2 (de-DE) | "12,34" | | "12,35" |
| 2.5m, 0 | "2" | 2m | |

So a total shown with `format_number(round(x, 2), ...)` and one shown with `format_number(x, ..., 2)` can differ by a cent. Its guide entry says it writes "the same way" as `money.format`, and that one "the same as `Intl.NumberFormat`". (`money.format` does not round, as amounts already have their currency's digits, so it is right.)

**Suggestion:** round half away from zero, from the float's shortest decimal, as `Intl` does by default (`roundingMode: "halfExpand"`); or take a mode as `round` does, and say which is the default.

### Q-370. In the browser, a place in the markup whose expression fails keeps showing its old value

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser, after clearing the booking's date field | 1 |

When the expression behind a place in a component's markup stops, the browser logs it and leaves the place as it was. A cleared `<input type="date">` makes `day` `""`, every `date` function stops on that, and the page goes on showing the last good day beside an empty field. The server, rendering the same state, would answer 500:

```quillo
component Due() {
    state day = "2026-11-20"
    <div>
        <input type="date" bind:value={day}/>
        <p>{date.format(day, "dddd D MMMM")}</p>
        <p>{len(day)}</p>
    </div>
}
route GET / = <html><body><Due/></body></html>
```

Pick the 21st, then clear the field: the first `<p>` still says "Saturday 21 November", and the second says 0. The console has `` quillo: `date.format` expects a date or a date-time as argument 1, got "" ``. In the app, the booking hint went on naming a day no longer chosen.

**Suggestion:** empty the place, or under `--watch` show the error there. Text that is no longer true reads as the page's answer.

### Q-371. A state set from a prop carries its value twice in the page

| Kind | Detected by | Attempts |
|---|---|---|
| perf | the scale test, by the page's size | 1 |

`state items = initial` is the guide's own pattern (`Notes(initial)`), and it puts the list in the island twice: in `data-p` as the prop and in `data-s` as the state. On the app's page of 2,000 appointments that is 819,257 and 819,206 bytes of a 2,076,299-byte page (53 KB gzipped):

```quillo
let rows = for i in 0..1000 => { id: i, text: `row number ${i}` }
component Copy(initial: list) {
    state items = initial
    <ul>{for r in items => <li key={r.id}>{r.text}</li>}</ul>
}
component Read(initial: list) { <ul>{for r in initial => <li key={r.id}>{r.text}</li>}</ul> }
route GET /copy = <html><body><Copy initial={rows}/></body></html>    // 155,956 bytes
route GET /read = <html><body><Read initial={rows}/></body></html>    // 91,157 bytes
```

**Suggestion:** when a state still holds the very value of a prop at render time, write a reference to it in `data-s` (`{"$prop": "initial"}`), not a second copy.

### Q-372. Every page with a component carries every translation of its locale, inline

| Kind | Detected by | Attempts |
|---|---|---|
| perf | reading `src/i18n.rs`, then a measure | 1 |

A page with any component gets `window.__quillo_i18n` in a `<script>`: every key of the default locale and of the page's own, whether or not a component uses it. It is written into each page, so the browser never keeps it. In the app, sign-in, invoice and reminder texts ride on the booking page: 2,865 of its 10,765 bytes. A product's 2,000 keys make a one-button page about 100 times the size:

```sh
# locales/en.json and fr.json: 2,000 keys of about 60 characters in 40 groups
echo 'translations("locales")
component Like() { state n = 0
  <button @click={() => { n += 1 }}>{t("section0.key0")} {n}</button> }
route GET / = <html><body><Like/></body></html>
route GET /plain = <html><body><p>{t("section0.key0")}</p></body></html>' > main.qwe
curl -s 127.0.0.1:18311/ | wc -c           # 144876 (13,659 gzipped)
curl -s 127.0.0.1:18311/plain | wc -c      # 1413
```

**Suggestion:** send only the keys the page's bundle can ask for (a `t("literal")` is known when it compiles, and a key made from parts by its start), as a file keyed by locale and content (`/_quillo/t/<hash>.js`), as Q-068's fix did for CSS.

### Q-373. A quote in an uploaded file's name arrives as `%22`

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | curl | 1 |

HTML's multipart encoding writes `"` in a file's name as `%22`, CR as `%0D` and LF as `%0A`. Chrome and curl both do this, and Quillo keeps the escapes, so a lab result named `résumé "final".pdf` is listed as `résumé %22final%22.pdf`:

```sh
curl -F 'docs=@a.txt;filename="résumé \"final\".pdf"' 127.0.0.1:18305/up   # "filename":"résumé %22final%22.pdf"
```

**Suggestion:** decode those three escapes in `filename` and in field names, and nothing else, as the HTML standard defines them.

### Q-374. A decimal kept in the session comes back as text, and `+` joins it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe | 1 |

`set_session` writes the session as plain JSON, so `12.50m` comes back as `"12.50"`, in the same request too. Then `+` joins text, `sum` joins it from 0, and `*` stops:

```quillo
route POST /cart { set_session({ total: 12.50m }); "ok" }
route GET /cart {
    let s = session()
    { plus: s.total + 1m, sum: sum([s.total, 1m]) }      // {"plus": "12.501", "sum": "012.501"}
}
```

`quillo check` passes it. Components already carry decimals across as `{"$dec": "12.50"}`.

**Suggestion:** write a session's decimals, and ints past 2^53, with the same marks the islands use.

### Q-375. `date.at` cannot name the second of a repeated time, so an hour of the night cannot be booked

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | the urgent care's slots on 2026-11-01 | 1 |

When clocks go back, `date.at` gives the first of the two times a wall-clock time names, as the guide says, and nothing asks for the second. New York's urgent care, open all night, offers 01:00 and 01:30 EDT and then 02:00 EST. The hour from 06:00Z to 07:00Z (01:00 to 02:00 EST) cannot be offered by local time at all. A time skipped going forward comes back moved, and nothing tells the caller it was moved: two local slots become one instant, and the app removes the copies with `unique`.

**Suggestion:** an option, as Temporal's `disambiguation`: `date.at(day, "01:30", zone, { repeated: "later" })`, and `{ skipped: "error" }` for code that must know.

### Q-376. The value of `<input type="datetime-local">` cannot be read, and a pattern reads it as UTC

| Kind | Detected by | Attempts |
|---|---|---|
| limitation, docs | a probe of HTML's input values | 1 |

`date.parse` refuses `"2026-10-27T14:30"`, the text a `datetime-local` input gives, with or without seconds. It also refuses an HTTP date (`Tue, 27 Oct 2026 14:30:00 GMT`). A pattern does read it, but as UTC, so a wall time in New York becomes an instant four hours early with no word:

```quillo
let v = "2026-10-27T14:30"                                   // from <input type="datetime-local">
date.parse(v) ?? "ERR"                                       // ERR
date.parse(v, "YYYY-MM-DD[T]HH:mm")                          // 2026-10-27T14:30:00Z: as UTC
date.at(slice(v, 0, 10), slice(v, 11, 16), "America/New_York")   // 2026-10-27T14:30:00-04:00: right
```

**Suggestion:** let `date.at(v, zone)` take a local date-time as one text. Say in Dates and times what each of HTML's inputs gives (`date`, `time`, `datetime-local`, `month`, `week`) and the call that reads it.

### Q-377. `ask()`'s own buttons are English on every page

| Kind | Detected by | Attempts |
|---|---|---|
| silent, small | browser, on the Spanish page | 1 |

On a page in Spanish, `ask(t("mine.cancel") + "?", { tone: "danger" })` shows "Cancelar?" over the buttons "Cancel" and "OK". The defaults ignore `locale()`, and in the app "Cancel" is both the patient's action and the dialog's way out. The app can pass `ok` and `cancel` itself, but nothing says it must.

**Suggestion:** take the defaults from the page's locale (keys such as `quillo.ok` and `quillo.cancel` in the program's translations, or a small table of its own), or say in the guide that a translated page passes both.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `range(start, end, step)` for slots | Python's `range` | `` `range` takes 2 arguments, got 3 ``; counted the slots instead, `for i in 0..n` | 1 check |
| `pad_start(str(n), 2, "0")` | JavaScript's `padStart` | no such builtin (the guide says there is no padding); a private `two(n)` | 1 lookup |
| `db.exec(sql, ...row)` | JavaScript spread in a call | Q-359's message; wrote out `r[0]` to `r[7]` | 1 check |
| `route GET / ? { done: str? }` | an optional type, as annotations take one | `` expected `,` or `}` in record pattern, found `?` ``, which does not say that a pattern takes `= default`; read `request.query.done` | 1 check |
| `route GET /invoices/:b(int).csv` | Express and Rails take a suffix after a parameter | `` expected `=` or `{` to start the route body ``; moved the name to a piece of its own, `/invoices/:b(int)/invoices.csv` | 1 check |
| `with_locale` in a helper a click also calls | it reads as plain code | Q-347: check ok, run refused | 1 run |
| `locale_from((r) => r.query.lang)` only | my own mistake: forgot the account's setting | Ana's Spanish account was served English; `r.query.lang ?? me().locale` works, a database read in the rule included | 1 browser run |
| `{ files: { docs: [map] } }` | Q-200's fix for `[str]` | Q-365 | 1 curl |
| times compared as text | the guide's advice | Q-368, with a client's `.000Z` | 1 call |
| `state items = rows` | the guide's `Notes(initial)` | Q-371 | the scale test |
| Two `Cookie` headers in one curl (`-b` and `-H`) | my own mistake | the session was not read: only the last `Cookie` header is, as the guide says | 1 probe |

## Limitations

- **Cron is UTC.** "07:00 at each branch" moves against UTC twice a year in three of the five zones. The app runs an hourly job and acts where the local hour is 7. A `zone` option on `schedule` would say it directly.
- **Dates and money in Arabic and Japanese** are written as in English (documented): Omar's Arabic reminder reads "Monday, November 2, 2026", and an Arabic page shows `₱1,682.24`. No call writes Arabic-Indic digits, and a `{count}` in a translation is never written in the locale's digits or grouping. The app passes the number for the plural form and the text apart: `t(key, { count: n, n: format_number(n, loc, 0) })`.
- **`format_number` takes exact digits only:** `format_number(10, loc, 3)` is `10.000`. `Intl`'s separate least and most digits would give `10` and `8.875` from one call.
- **A download keeps its name only through its path.** `file()` takes `cache` alone, so the name goes in the address: `/results/:id(int)/*name`.
- **No CSV writer.** The app quotes cells and puts `'` before `=`, `+`, `-` and `@`, so a patient named `=HYPERLINK(...)` is not a formula in a spreadsheet. Excel also wants a byte-order mark to read the Arabic names as UTF-8.
- **The visitor's zone outside a request.** The `tz` cookie the guide suggests reaches routes, not jobs: a reminder can only use a zone the app stored, and the first page is made before the cookie exists (documented).
- **A file's kind comes from its bytes, by hand.** The app reads the first five characters of a PDF, and saves an image to a temporary path so that `image.info` can look at it, since `image.info` takes a path and not an upload.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| `date.at` over the changes in New York, Sydney, London, Berlin and Lord Howe (30 minutes), and Samoa's skipped 2011-12-30 | right on the server, and the same in the browser |
| 64 date, money and number cases, server against a component | 61 the same; the other three as documented (a hidden error's text, `1.0` outside a component) |
| A float in a translation's value inside a component (`count: 4.0`) | "4" on both sides, nothing remade |
| Two patients booking one time at once | one booked; the other told "Someone has just taken that time" in their language |
| A time between two slots, or past the doctor's day | refused |
| `day: date` on a `server fn` | refuses `2027-02-30` and a date-time with a 400 |
| Cached views reading `session()`, `cookie()` and `locale()` | kept apart per visitor and per language |
| `set_session`, then `session()` in the same request; `end_session` likewise | sees the new value |
| Cookies with `%ZZ`, a bare `%` or `%E2%9C%93` | read as sent, `✓` decoded |
| Pointing at the `GET /lang/es` link, which sets a cookie | the prefetch is refused (`503 prefetch`); the click works |
| `..` in a `save_upload` path, a Windows path as a filename | refused; `report.pdf` |
| "No file chosen" (an empty part with `filename=""`) | not in `request.files` |
| An HTML file renamed `.pdf`; a real PDF and PNG | refused by the app's byte check; kept |
| Another patient's result or invoice; a patient at `/desk` | 404; 403 |
| `../../etc/passwd`, `/etc/localtime` or a 4,000-character zone in the `tz` cookie | refused by `date.valid_zone` |
| Arabic's six plural forms (0, 1, 2, 3, 11, 100) on both sides; `dir="rtl"` | right |
| The reminder job | written at the five-minute mark, in Spanish and Arabic, once each |
| 2,000 appointments for one patient | 26 ms to render; resumed with nothing remade; a cancel in 31 ms |
| Tax: 8.875% on USD 187.50, 10% in yen | $16.64 (half up), ¥580, no decimals |
| `money.format` in en-PH, de-CH, en-IN, fil-PH | as `Intl` writes them |

## Not tested

PostgreSQL and MySQL (all on SQLite), `--instances`, the phone apps, Hebrew, a real spreadsheet opening the CSV, a screen reader on the right-to-left pages, and the visitor's zone as Safari and Firefox report it.
