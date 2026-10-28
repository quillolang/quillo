# Feedback from building a booking platform in Quillo

| | |
|---|---|
| App | [stays/](../../app/stays), "Quillo Stays": 2,493 lines of Quillo in 14 files, 167 of SQL, plus the Quillo UI kit's 37 components (3,405 lines, vendored unchanged from quillolang/components 9713616). Places to stay in five cities (Kyoto's in yen): search with filters and a map, a place's page with a live calendar, the price worked out in the browser as the server will charge it, the weather for the dates (Open-Meteo), places to eat and see around it on a map (Overpass, with Photon when Overpass fails), activities nearby with time slots and places left. Nights held 10 minutes while the guest pays, requests the host has a day to answer, cancelling with refunds by policy (worked out from the check-in time in the place's own time zone), reviews, saved places. For hosts: a dashboard (requests with their deadline, earnings by month in each currency, nights booked), a calendar to close, open and price nights, a new place found on the map. Outside answers are kept in the database and every call is logged (/status). |
| Quillo | 0.5.0 (ae6a6e5) |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; probes of each outside service and of the risky parts before building; headless Chrome with up to five people signed in at once, each in a browser of their own (`app/stays/tests/book.mjs` 31 checks, `host.mjs` 27); hostile calls to every server fn, 20 holds of the same nights at once, 20 people at once for 8 places (`hostile.py` 78); the real services online, Overpass pointed at a dead server and at one that hangs; `feedback/tools/mismatches.mjs` on every page, signed out, as a guest and as a host; a phone's width |

What worked well: the hard parts of a booking platform. Two guests watching the same place
see each other's holds close nights on their calendars within a second (`live` with a topic
per place), and a host's dashboard shows a request as it is made; 20 holds of the same
nights sent at once gave exactly one booking (a unique key on a night and a place, in
`db.transaction({ retries: 5 })`), and 20 people at once for an activity's last 8 places
got exactly 8. `money` gave the same totals on the server and in the browser for euros and
yen (no cents), to the cent, once Q-045 was worked around, and `money.allocate` split a stay
into nights for refunds without losing a cent. Outside services were easy to wrap: a slow
Overpass (34 s) held up nobody else (other pages answered in 8 to 17 ms meanwhile), a
second search for Sintra answered from the cache in under a millisecond, and Photon took
over in 2.8 s when Overpass was down. `escape` kept a host's title with markup in it
harmless in Leaflet's tooltips. `quillo check` caught five of my mistakes before they ran,
two of them real bugs (a `state map` hiding `map` in the component that called it, a type
named like the kit's `Card`).

What it cost: four silent failures in the browser (Q-045, Q-046, Q-048, Q-050), each found
only by a test or by looking at a page, and one under `--watch` that breaks the kit's
`<Rating>` everywhere (Q-047); and every component a booking site needs most (a map, a
calendar of nights that are taken, a place search) had to be written by hand (see
[Components the app needed](#components-the-app-needed)).

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-027 | No date module and no time zones: lib/dates.qwe is 126 lines of calendar arithmetic (days from 1970 and back, weekdays, nights between two days), and the EU's summer time by hand, because refunds turn on the check-in time in the place's own zone (Lisbon was UTC+1 on the 24th and UTC+0 on the 26th of October 2026) | – |
| Q-028 | A place's live activities (6.5 KB) go again to every page that shows any of them when one slot fills; the place's page itself is 1.6 KB. Fine at this size | – |
| Q-006 | VS Code still shows `unknown name` for every name from another file (ui/search.qwe: `Shell`, `Button`, `search_stays`...), though `quillo check` on main.qwe is clean | – |
| Q-041 | Avoided: every part of a page chosen with `? : null`, never `&&`, from the start | 0 |
| Q-044 | Avoided: `floor`/`ceil` wherever a division meets an int | 0 |

## New issues

### Q-045. A top-level `let x = 0.12m` is a dec on the server and text in the browser

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `mismatches.mjs`: "money.mul expects a dec or an int as argument 2, got str" in the console of one page | 2 |

lib/pricing.qwe had `let SERVICE_FEE = 0.12m` (and the weekly and monthly discounts). The
booking widget works out a stay's price in the browser with the same `quote` function the
server charges with, and on the page it showed no price at all: the first `money.mul` threw,
and only the console said so. In browser code a top-level decimal, alone, in a list or in a
map, is the text `"0.12"`; the same literal inside a function or a component is a dec. The
server renders `dec`, the browser then shows `str`, and no mismatch is reported.

```quillo
let FEE = 0.12m

component Fee() {
    let local = 0.12m
    <p id="types">{type_of(FEE)} {type_of(local)}</p>       // server: "dec dec", browser: "str dec"
}
```

Repro: [Q-045-top-level-dec-is-text-in-the-browser.qwe](../repros/Q-045-top-level-dec-is-text-in-the-browser.qwe).

**Workaround in the app:** the rates are functions (`fn service_fee() = 0.12m`).

**Suggestion:** send top-level decimals to the browser as decimals, as constants of any other
type already are; until then, `quillo check` could refuse a top-level decimal that browser code reads.

### Q-048. A range input whose value the browser corrects is taken on resume for typing: its `@input` runs with nobody touching it

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the browser test (4 places in the server's HTML, 3 on the page), then printing each filter's count on both sides | 3 |

The search's price slider (the kit's `RangeSlider`) had max 152 (the dearest night, €151.67,
rounded up) and step 5. The browser makes such an input's value 150, the nearest step. When
the component is resumed, Quillo compares the inputs' values with what it rendered, takes
the difference for something typed before the script came, and dispatches an `input` event
(`isTrusted: false`). The slider calls `on_change(0, 150)`, and the dearest place is
filtered out of the results before anyone does anything. Nothing is counted as made again.

```quillo
component Pick() {
    state v = 152
    state events = 0
    every(3600000, () => null)
    <div>
        <input id="r" type="range" min="0" max="152" step="5" value={v} @input={(e) => { v = int(e.value); events += 1 }}/>
        <p id="out">{v} after {events} events</p>         // "150 after 1 events", untouched
    </div>
}
```

Repro: [Q-048-a-value-the-browser-corrects-is-replayed-as-typing.qwe](../repros/Q-048-a-value-the-browser-corrects-is-replayed-as-typing.qwe).

**Workaround in the app:** the slider's top is a whole number of steps.

**Suggestion:** replay only what a person did (a value different from the one the browser
itself settled on at parse time, or only for focused or `isTrusted` changes); a range
input's snapping, clamping into min..max and a select whose value is not an option are the
browser's, not the visitor's.

### Q-046. A component that runs by itself inside another one does not start when the page is idle

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the browser test (the search map had no tiles); a probe that first passed by accident | 3 |

The guide: "A component that runs by itself (`every`, `live`, `js`, `@mount` in its code or
markup) or that is the page loads as soon as the page is idle." The search's map is drawn in
an `@mount` in `<StaysMap>`, inside `<SearchApp>`. The map stayed grey, no tiles and no
pins, until the mouse passed over the page. The same for `live(...)` and `every(...)` in the
inner component. The first repro passed: its outer component was `<body>`'s own child (an
"app", which loads at once), as small examples usually are.

```quillo
component Inner(id: str) {
    state said = "waiting"
    <p id={id} @mount={(el) => { said = "mounted" }}>{said}</p>
}
component Outer() {
    state n = 0
    <div><button @click={() => { n += 1 }}>{n}</button><Inner id="inner"/></div>
}
route GET / = <html><body><main><Outer/></main></body></html>     // "waiting" until touched
```

Repro: [Q-046-mount-in-a-nested-component-waits-for-a-touch.qwe](../repros/Q-046-mount-in-a-nested-component-waits-for-a-touch.qwe).

**Workaround in the app:** `every(3600000, () => null)` in `SearchApp`, which makes the outer
component one that runs by itself.

**Suggestion:** when deciding what loads at idle, count what runs by itself anywhere inside
an island, not only at its top.

### Q-050. A Quillo function handed to a JavaScript library throws when the library calls it with more arguments than it declares

| Kind | Detected by | Attempts |
|---|---|---|
| silent | probing Leaflet before building: "QuilloError: `<fn>` takes 0 arguments, got 1" in the console at each click | 1 |

`marker.on("click", () => { picked = id })`: Leaflet calls it with the event, the function
takes none, and it throws inside Leaflet, so the click does nothing. JavaScript callbacks
ignore extra arguments, and libraries are written for that (Leaflet, Chart.js and the DOM all
pass an event or more). For `@event={fn}` the guide says the function *may* take the event;
a function handed to `js` must.

Repro: [Q-050-js-calling-back-with-extra-arguments-throws.qwe](../repros/Q-050-js-calling-back-with-extra-arguments-throws.qwe).

**Workaround in the app:** every function given to Leaflet takes `(e)`.

**Suggestion:** a function that crosses into JavaScript accepts extra arguments, as `@event`
handlers do; or say so in "JavaScript libraries".

### Q-047. Under `--watch`, markup kept in a top-level `let` compiles to code that does not parse

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `mismatches.mjs` with `--watch`: "SyntaxError: Invalid or unexpected token" and `$b117 is not defined`; reading /_quillo/src/.../program-48.js | 2 |

The kit's rating.qwe keeps its star as `let rating_star = <svg>...</svg>` at line 59. In
development, each function goes to the browser in a file of its own, and this one came as
`59return W.t($b117, []);`: the line number glued to `return`. Every `<Rating>` on the page
(the reviews) stopped working. Without `--watch` the bundle is right.

```quillo
let star = <svg viewBox="0 0 24 24"><path d="M12 2l3 7h7l-6 4 2 7-6-4-6 4 2-7-6-4h7z"/></svg>

component Stars(n: int) {
    state k = n
    <button id="b" @click={() => { k += 1 }}>{for i in range(0, k) => <span key={i}>{star}</span>}</button>
}
```

Repro: [Q-047-markup-in-a-top-level-let-breaks-watch-mode.qwe](../repros/Q-047-markup-in-a-top-level-let-breaks-watch-mode.qwe) (`flags: --watch`).

**Workaround in the app:** none (the kit is vendored unchanged); the tests run without `--watch`.

### Q-049. `http.get` ignores options it does not know, and cannot be told to give up sooner than 60 s

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a probe: an 8 s delay with `{ timeout: 2 }` took 8.3 s; then reading src/fetch.rs | 2 |

Overpass answers places around a stay in 34 s when it is busy (most of the time this week).
`http.get(url, headers, { timeout: 5 })` waits the 34 s all the same: the options take only
`redirects`, `public_only` and `cache`, and anything else is dropped without a word, by
`quillo check` and at run time. The limits are Quillo's own, 10 s to connect and 60 s in all,
and the guide does not give them. So a service that hangs costs a page up to a minute, and a
fallback (Photon) only starts when the first one fails outright: 7.5 s when Overpass was
pointed at a host that drops connections.

Repro: [Q-049-http-options-unknown-key-ignored.qwe](../repros/Q-049-http-options-unknown-key-ignored.qwe) (`quillo check` passes).

**Workaround in the app:** nothing that renders on the server waits for a service: the
weather and places are asked for in an `@mount` after the page shows, the activities' weather
is read from the cache only (`forecast_kept`), and each answer is kept (a place for a month,
the weather for half an hour, places around for a day).

**Suggestion:** a `timeout` option (and say the defaults in "HTTP client"); unknown option
keys an error in `quillo check`, as unknown fields are elsewhere.

### Q-051. `xs | find(f) != null` is `xs | (find(f) != null)`, and the errors do not say so

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

`{act.slots | find((t) => t.id == slot) != null ? <form/> : null}` gave three errors:
"argument 1 of `find` must be list | null, got fn", "missing argument `arg2` in call to
`find`" and "cannot call `this function`: it is bool, not a function". Brackets fix it:
`(act.slots | find(...)) != null`. JavaScript's `xs.find(f) != null` needs none, so this will
be written often.

Repro: [Q-051-pipe-binds-looser-than-comparison.qwe](../repros/Q-051-pipe-binds-looser-than-comparison.qwe).

**Suggestion:** when the right side of `|` is a comparison or another operator, say that `|`
binds looser and suggest the brackets; and name the function in "cannot call".

### Q-052. An address `public_only` refuses is reported as a failed DNS lookup

| Kind | Detected by | Attempts |
|---|---|---|
| loud | the log, while pointing the app at a dead Overpass | 1 |

`http.get("http://127.0.0.1:9/x", {}, { public_only: true })` gives
`err("http: http://127.0.0.1:9/x: Dns Failed: resolve dns name '127.0.0.1:9': 127.0.0.1 is not
an address on the public internet")`. Nothing was looked up, and "Dns Failed" sends one to
check DNS. The refusal is right, and instant (203.0.113.1, a documentation address, too).

Repro: [Q-052-refused-private-address-said-as-dns-failure.qwe](../repros/Q-052-refused-private-address-said-as-dns-failure.qwe).

**Suggestion:** "refused by public_only: 127.0.0.1 is not on the public internet".

### Q-053. A database error is only its text

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | a probe of a unique key's failure | 1 |

`db.exec(...)` on a night already taken gives `err("db: UNIQUE constraint failed:
booking_nights.listing_id, booking_nights.day")`: text only. Telling "someone has just booked
that night" from "the database is down" means matching the driver's words, and they differ:
SQLite says "UNIQUE constraint failed", PostgreSQL "duplicate key value violates unique
constraint", MySQL "Duplicate entry". The app checks for `"UNIQUE"` (lib/stays.qwe,
`hold_stay`), which would be wrong on PostgreSQL.

**Suggestion:** a kind on database errors (`unique`, `foreign_key`, `not_null`, `busy`), with the
constraint's name, the same for the three databases.

## Components the app needed

The app uses 37 of the kit's 102 components: Navbar, Button, Card, Badge, Alert, EmptyState,
Skeleton, Spinner, TextField, TextArea, Select, Switch, NumberInput, RangeSlider, ChipGroup,
SegmentedControl, DateRangePicker, Rating, Carousel, Lightbox, Avatar, DescriptionList, Steps,
Countdown, Stat, BarChart, CalendarHeatmap, Timeline, StatusDot, and their parts. These did
the work: the checkout's Steps and Countdown (whose `on_end` shows the hold ran out), the
host's Stat, BarChart (a chart for each currency) and CalendarHeatmap, Carousel and Lightbox
for pictures. What a booking site needed most was missing, most needed first:

| Needed | What was done instead | Cost |
|---|---|---|
| **Map**: markers from a list, a price on each pin, a picked pin, the view following new results, a draggable pin for "where is it", the area in view as a search filter, clustering | Leaflet by hand, through `js()` and `@mount`, three times (search, around a place, a new place): about 90 lines, and the two silent failures of crossing into JavaScript (Q-046, Q-050) | high |
| **Nights that are taken, in the date range picker**: `disabled_dates` on DateRangePicker (DatePicker has it, and `disabled_weekdays`), a range not allowed across them, a price or a mark under each day, the fewest nights | `<Availability>` by hand (45 lines): three months of days, taken ones crossed out, two clicks for a stay; the widget says "Not free: ..." when the picker's range crosses taken nights | high |
| **Place search with suggestions**: a Combobox whose options are loaded as one types (`on_input`, with a delay between keys), for Nominatim or Photon | a plain TextField, searched on submit; SearchInput has `on_input` but no list, Combobox no loading | medium |
| **A month calendar to edit**: days with a state and a label each (price, guest), several picked (shift-click for a run), actions on what is picked | `<HostCalendar>` by hand (60 lines); CalendarHeatmap only shows | medium |
| **Time slots**: times with places left, the full ones disabled, one picked | buttons by hand (`.slot`) | low |
| **Price breakdown**: lines with a label and an amount, discounts in another colour, a total | a list by hand, twice (widget, checkout) | low |
| **Guests**: adults, children, infants and pets in a popover, with a total | one NumberInput | low |
| **Card input**: digits grouped as typed, the brand, MM/YY | three TextFields, checked on the server (Luhn, expiry) | low |
| **A bar at the bottom on a phone** for the page's main action ("€95 a night · Reserve") | none: on a phone the booking widget is far below the pictures | low |

What made the kit's components harder to use than they need be:

- **No `id`, `data-*` or other attributes pass through.** `<Button id="reserve">` is an error
  ("<Button> has no parameter `id`"), as on Alert: tests and links to a part of a page need
  one. The app uses `class` instead. A view cannot take "the other attributes" and pass them
  on, so each component would have to list them.
- **Inputs take their value once.** DateRangePicker keeps `start` and `end` in its own state,
  so when the place's calendar picks the dates, the picker does not follow: it needs
  `key={`${start}|${end}`}` to start again, which then must not happen while the picker itself
  is half way through a range (the app passes on only complete ranges). The kit's comments
  do not say which components follow their props.
- **RangeSlider calls `on_change` at each step of a drag** and has no "done" event, so a filter
  that asks the server would ask it at every pixel. The app filters the price in the browser.
  Its max not being a whole number of steps also meets Q-048.
- **One namespace for everything.** The kit's `Card`, `Tab`, `Tooltip`, `Badge` take those
  names for the whole program: `type Card = { number: str, ... }` for a payment card was
  "defined twice" (a clear error). A prefix (`UiCard`) or a module name would leave room.
- The kit's `let rating_star = <svg>` meets Q-047 under `--watch`.

## Where the AI struggled

| What I wrote first | Why it seemed right | What happened | Cost |
|---|---|---|---|
| `xs \| reduce((m, r) => ..., {})`, four times | JavaScript's `reduce(f, init)` | "argument 3 of `reduce` must be fn, got map": clear; the guide's table says `reduce(xs, init, f)` | 1 check |
| `entries(map)`, `merge(a, b, c)` | other languages' helpers | unknown name; "merge takes 2 arguments" | 1 check |
| `re.match(text, pattern)` and `m[1]` | Python and JavaScript | "`re` has no function `match`": it is `re.find`, and groups are `m.groups[0]` | 1 run |
| `json.stringify(r) \|> len` | another language's pipe | "the pipe is now a single `\|`" | 1 run |
| `xs \| find(f) != null` | JavaScript's `xs.find(f) != null` | three errors about `find`'s arguments (Q-051) | 1 check |
| `state map = null` for a Leaflet map, then `keys(x) \| map(...)` in the same component | the obvious name | warned: the state hid the builtin `map` there; a real bug | 1 check |
| `let last = (k) => ...` in pricing, `fresh(ask)` with a parameter `ask` | short names | warned: they hide `last` and the dialog `ask` | 1 check |
| `sign_in` declared `with db`, calling `crypto.hash_password` | hashing feels pure | "needs the random effect" (salts are random) | 1 check |
| `type Card` for a payment card | the natural name | "defined twice", with the kit's `<Card>` | 1 run |
| `card: map` as a server fn's parameter | a map is what the form sends | a number for the card's number reached `card_problem(number: str)`: a 500. A record type (`PaymentCard`) makes it a 400 before the code runs | 1 test |
| `is_err(b) ? b.error : ...` after `money.mul` | `money.mul` gives an error value for a bad argument | `quillo check`: "this map has no field `error`": it takes `money.mul` to give money only | 1 check |
| `<Button id="reserve">`, `<Alert id="booked">` | test hooks | no such parameter (see above) | 1 check |
| `route GET /pics/city/:hue(int)` for `/pics/city/20.svg` | the hue is the name | 404: `20.svg` is not an int; my bug | 1 scan |
| `@mount` for the map inside the search component | the guide's own example | blank map until touched (Q-046); the first probe passed because its outer component was `<body>`'s child | 3 probes |
| The slider's max as the dearest price rounded up | the range the data has | a place left out with nobody touching anything (Q-048); first suspected the filters' code | 3 runs |
| `localhost` in the Node test tools | as everywhere | Node's `fetch` tried IPv6 (`::1`); Quillo listens on 127.0.0.1 | 1 run |
| `<script>` with `{` in a repro's page | HTML | braces in markup are expressions; `{raw(JS)}` | 1 run |
| A User-Agent naming a contact email for Nominatim | its policy asks for contact details | 403 from Nominatim for that text; a plain "QuilloStays/0.1 (...)" works. Quillo's default `quillo/0.5.0` is not one its policy accepts either | 2 probes |

## Limitations

What is missing by design or not built yet, and what the app does instead.

- **No date module, no time zones** (Q-027): lib/dates.qwe, with the EU's summer-time rule
  by hand; Kyoto has none. A city's zone is a table, not a database.
- **No exchange rates:** the price filter is in the currency most places found have; places
  in another currency are not filtered by it. Earnings are a chart for each currency.
- **No trigonometry in SQLite** (no `acos`, `sin`, `radians`): distances (haversine) are
  computed in Quillo, after a bounding box in SQL.
- **No rate limits** (removed in 205d7cd): Nominatim's one call a second is kept by reading
  the time of the last call from the call log, and sleeping.
- **No way to load state after the page shows:** a `state` initializer runs while the server
  renders, so anything slow (Open-Meteo, Overpass) goes in an `@mount` that calls a server fn,
  with a Skeleton meanwhile.
- **No debounce** for handlers: the price filter is applied in the browser instead of asking
  the server at each step.
- **Scheduled jobs run each minute at best:** a hold that runs out is shown at once by the
  checkout's Countdown, but its nights come free for others at the next minute's sweep (and
  before each new hold, which sweeps first).
- **Overpass is the only free source of places** and is overloaded: answers are kept a day,
  Photon (by kind, 6 calls) stands in when it fails.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| 20 holds of the same nights at once, by 20 guests | one hold, 19 "Someone has just booked the night of ..."; four nights in the table, once each |
| 20 people at once for an activity's last 8 places | 8 booked, `taken` never over `capacity` (`update ... where taken + ? <= capacity`) |
| A hold that runs out while the guest looks at the checkout | the Countdown's `on_end` says so at once; the minute's sweep marks it expired, and the page follows live (47 s later); paying then is refused |
| Two guests on one place | a night held by one is crossed out on the other's calendar within a second; picking across it says "Not free" and offers no price |
| A host's request answered on the dashboard | the guest's checkout turns to "Booked", live; a declined one shows on the guest's trips, live; one past its day cannot be accepted |
| The browser's price and the server's | the same, to the cent, for 3 nights in Lisbon (with tourist tax) and in Kyoto (yen, no cents) |
| A host's new price or closed nights | the guest's page shows the new total, or "Not free", live |
| Cancelling: flexible, moderate, strict; a day before, during the stay | refunds by `money.allocate`, adding up; the policy's line checked again at the moment of cancelling, not at the preview |
| A title with `<img onerror>` in it | escaped in the page, the cards and Leaflet's tooltips (through `escape`); the script never ran |
| Server fns out of turn: signed out, as another guest, as another host | refused, each with its own words (20 checks) |
| An id as text, an argument missing, a card's number as a number | 400 before any code runs (with a record type for the card) |
| Dates: 30 February, the past, a year ahead, leaving before arriving, fewer nights than the place takes | refused with words a guest understands |
| Yen with cents, euros with three decimals, a price that is not a number | refused by `money.of` |
| A slow Overpass (34 s) | other pages answered in 8 to 17 ms meanwhile |
| Overpass down (refused, or hanging) | Photon's places within 2.8 s (refused) or 10.5 s (hanging) |
| The same place searched twice | the second answer from the cache, under a millisecond; the call log shows one call |
| Every page, signed out, as a guest and as a host (`mismatches.mjs`) | taken up, never made again, no errors (without `--watch`: Q-047) |
| A phone's width | no page scrolls sideways; the search map goes above the results |
