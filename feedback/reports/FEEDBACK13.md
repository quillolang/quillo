# Feedback from building two-shift scheduling for a factory in Quillo

| | |
|---|---|
| App | [shifts/](../../app/shifts), "Quillo Shifts": 24 production lines, each with crews A, B and C of 4 to 10 operators (506 in all) on 12-hour day and night shifts, rotating two days, two nights, two days off; operators ask for leave, their line's supervisor approves it (not below the line's minimum without a reason), and the manager sees every line's manpower for next week or a month, live. 800 lines of Quillo in 10 files, a migration, a stylesheet and a browser test |
| Quillo | 0.5.0 (add81dc); every finding checked again on 7989489, the newest commit, where all five are still open |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; probes of dates, zones and live data first; [tests/leave.mjs](../../app/shifts/tests/leave.mjs) in headless Chrome, with an operator, a supervisor and the manager in three browsers at once (18 of 18 pass); `tools/mismatches.mjs` on every page as each role (nothing made again); hostile calls to every `server fn`; 1,440-cell month grids; 23 pages open at once |

What worked well: the date module carried the whole domain. The same rotation code runs on
the server and in the browser with the same answers, the DST nights included (the night of
24 October 2026 in Europe/Berlin is 13 hours, on both sides, and 28 March's is 11), so the
operator's calendar and the leave form's count of shifts are worked out in the page with
nothing sent to the server. Records turned away what a hostile page sent (`kind:
"vacation"`, a number for a day, an extra `status: "approved"` field) before any of my code
ran. `db.transaction({ isolation: "serializable" })` made two approvals sent at the same
moment, each fine alone and together below the minimum, end with one approved and one
refused with the reason. `publish` inside a transaction waited for the commit and was
dropped on rollback. A live month grid of 133 KB was brought up to date after a decision
with a 421-byte patch. In the supervisor's list, the reason half typed into one request kept
its text and its focus while another request arrived and another left.

What it cost: the guide's own advice for live data (mark rows read in the loader) makes a
loader that runs for ever (Q-090). An app served on a factory's network over plain HTTP
cannot keep anyone signed in once it runs without `--watch`, with no word from the server
(Q-089). A live grid whose month was changed shows the old month under the new title until
the answer comes (Q-091).

## Seen again

None of the known issues. The nearest is the family of Q-084 (a write wakes every loader that
read what was written): here a table, which the guide documents. Before the app gave each
operator a topic of their own, one leave request with 20 operators' pages open caused 63
loader requests (40 `my_leaves`, 20 `my_balance`, 3 for the supervisors and the manager);
with the topic, 5. At 506 operators online that is about 1,500 requests for each request
made anywhere in the factory.

## New issues

### Q-090. A live loader that writes a table it reads runs again for ever

| Kind | Detected by | Attempts |
|---|---|---|
| silent (perf) | a probe of my own that counted runs, then the guide's pattern | 2 |

The guide (Live data) says what should happen when new data arrives for a viewer, "marking
posts or messages read", is done in the loader. A loader that does so writes the table it
reads; the write is announced, and wakes the loader that made it, which writes again. After
one message was sent the loader below ran 23,914 times in 6 seconds, about 4,000 a second,
from the second run on with an `update` that changed no row. The page shows the right
messages; the server's log fills with `POST /_quillo/rpc/inbox -> 200 (0.0ms)` and nothing
else says anything. A loader that assigns a global it reads loops the same way (18,000 runs
in 4 s, a probe's counter). The browser stops a loop of updates after 1,000 rounds; the
server has no such guard.

```quillo
server fn inbox() {
    db.exec("update messages set seen = 1 where seen = 0")      // 0 rows from the second run on
    db.query("select id, text from messages")
}
server fn send(text: str) = db.exec("insert into messages (text) values (?)", text)
component Inbox() {
    state msgs = live(inbox)
    <ul>{for m in msgs => <li key={m.id}>{m.text}</li>}</ul>
}
```

Repro: [Q-090](../repros/Q-090-loader-that-writes-what-it-reads-runs-forever.qwe) (about 4,000
runs a second on add81dc and 7989489).

**Workaround:** do the marking `unwatched`, or follow a topic of the page's own
(`watch(...)` and `unwatched` reads), so the loader's own write wakes nothing it follows.

**Suggestion:** a write made by a loader's run does not wake that run's loader for the same
page; a write that changed no row (`changes == 0`) is not announced; and the server warns,
as the browser does, when one loader is woken more than some number of times a second by its
own writes. The guide's advice needs the warning beside it until then.

### Q-089. Over plain HTTP at an address other than localhost, signing in never sticks

| Kind | Detected by | Attempts |
|---|---|---|
| silent | headless Chrome at the machine's LAN address | 1 |

A factory's kiosks and supervisors' tablets reach an app on its network, often over plain
HTTP (`http://10.0.0.5:8080`). Without `--watch` the session cookie is
`__Host-quillo_session=...; Secure`, which a browser keeps only over HTTPS or from
`localhost`/`127.0.0.1`. At `http://192.168.1.126:8933` the sign-in answered 303 to `/me`,
`/me` found no session and sent the browser back to the login page, with no error on the
page and none in the server's output: `POST /login -> 303`, `GET /me -> 303`. The same
program under `--watch` works (the cookie is then `quillo_session`, not `Secure`), so the app
works while it is built and stops when it is deployed. The guide says to serve over HTTPS
in production, under Cookies and sessions; nothing at the moment it matters says so.

Repro: [Q-089](../repros/Q-089-session-cookie-dropped-on-plain-http) (a script: serves on
`0.0.0.0`, signs in, shows `Set-Cookie: __Host-quillo_session=…; Secure` and an empty log).

**Workaround in the app:** none in Quillo code; serve it behind a proxy with TLS, or run with
`--watch`.

**Suggestion:** when the server listens on an address other than loopback without `--watch`,
say once at start that sessions need HTTPS in front; or, better, when a session is set on a
request that came over plain HTTP from another host with no `x-forwarded-proto: https`, log
that the browser will drop it. An option for networks that are trusted and plain
(`--plain-http-cookies`) would let a kiosk app say so on purpose.

### Q-091. A live loader whose arguments changed keeps showing the old answer, and nothing tells the component it is waiting

| Kind | Detected by | Attempts |
|---|---|---|
| limitation (silent) | a browser test that read the grid's title and its first column | 2 |

The manager's grid is `state grid = live(load_manpower, range[0], range[1])`, its range a
`let` of `mode` and `start`. Clicking "Later" changes the title at once (it reads `range`),
while `grid` keeps November's answer until December's comes: for a round trip the page says
"Tue 1 Dec – Thu 31 Dec" over November's 30 columns. On a slow line, or a server busy with a
month of 24 lines, a manager reads a month's manpower under the wrong month. The guide says
an answer for the old arguments is dropped, and that there is no loading flag around a
`server fn` call, but a live loader's call is not made by the component, so no state can be
set around it.

```quillo
server fn load_month(m: int) { sleep(300); { month: m, rows: [] } }
component Grid() {
    state month = 11
    state data = live(load_month, month)
    <p>month {month}, data for {data.month}</p>     // "month 12, data for 11" for 300 ms
}
```

Repro: [Q-091](../repros/Q-091-live-with-new-arguments-shows-the-old-value.qwe).

**Workaround (not yet in the app):** have the loader give back its arguments (`grid.from`) and
compare them with the ones asked for, dimming the grid while they differ.

**Suggestion:** a way to know a live state is waiting for the answer to new arguments, for
example `loading(grid)`, or the state taking `null` (as a navigation's state does) when
asked.

### Q-093. `date.valid` passes a date-time that the other date functions refuse with a runtime error

| Kind | Detected by | Attempts |
|---|---|---|
| loud | a probe before writing the leave form | 1 (avoided) |

A leave request's days come from a page as text. `date.valid("2026-11-20T08:00:00Z")` is
`true`, since it takes "a date or a date-time", but `date.add_days`, `date.days_between` and
the others take only dates, and a date-time there is a runtime error, not an error value: the
request ends with a 500, and `??` cannot catch it. (`date.weekday` takes either.) A record
cannot say that a field is a date (`start_day: str` is the nearest), so `LeaveRequest(...)`
does not check it either. The app checks each day with a pattern of its own before any date
function sees it (`is_day` in lib/rota.qwe).

```quillo
route GET /next ? { d: str } = date.valid(d) ? date.add_days(d, 1) : respond(400, "not a date")
// /next?d=2026-11-20T08:00:00Z -> 500
```

Repro: [Q-093](../repros/Q-093-date-valid-passes-what-date-functions-refuse.qwe).

**Suggestion:** `date.is_date(s)` (a date, not a date-time) beside `date.valid`, or `date.valid(s,
"date")`; and a `date` type for records, patterns and `server fn` parameters (`start_day:
date`), checked as `dec` is, so that `/leave/:from(date)` and `LeaveRequest(json)` refuse what
the date functions would.

### Q-092. A file that does not parse makes `quillo check` warn about every class it writes

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

I named a parameter `view` (a keyword) in ui/manpower.qwe. `quillo check` gave the right
error, `expected a parameter name, found view, which is a keyword: pick another name`, and
with it 17 warnings that `.wide`, `.seg`, `.grid`, `.we`, `.dn`, `.summary` and `.scroll`
"are styled, but nothing in the program writes" them: every class the file that failed to
parse writes. Q-033 made a parse error come first and keeps its names from being reported
unknown; the check for unused classes does not know about the failed file yet.

Repro: [Q-092](../repros/Q-092-parse-error-makes-unused-class-warnings) (`.grid` and
`.short`, written only in the file that does not parse).

**Suggestion:** with a file that did not parse, leave out the unused-class warnings (or
those for classes the file's text holds), as is done for unknown names.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `fn range_of(view: str, ...)` and `state view = "week"` | `view` is a plain word in most languages | parse error, plus 17 misleading warnings (Q-092) | 1 check |
| A probe's loader counting its runs in a global (`runs = set(runs, ...)`) | the quickest counter | the loader woke itself: 18,000 runs in 4 s (Q-090) | 1 probe |
| A counter written to the database instead | the same | the guide says a run that writes is never shared, so the probe measured no sharing at all | 1 probe |
| Counting `POST /_quillo/rpc/my_leaves` lines in the log as loader runs | the log looked like the runs | 40 lines for 20 pages looked like runs not shared; `print` showed one run, answered to every component that follows it | 3 probes |
| The browser test's "today" from the machine's clock (`+08`) | habit | the factory is in Europe/Berlin, where it was still the 28th: "today is marked" failed | 1 run |
| The seed's minimum crew of 5 for every line | a round number | crews of 4 made 112 shifts short in November; the minimum is now one less than the line's smallest crew | 1 run |
| `PATTERN[n % 6]` for the rotation | JavaScript habit | right by accident before the rotation's first day: `%` keeps the sign, and a negative index counts from the end, so `PATTERN[-2]` is item 4; `mod()` written instead, since a cycle day shown to people (`n % 6 + 1`) would be -1 | 1 probe (avoided) |
| `date.valid(s)` to check a day from a form | its name | it passes date-times, which `date.add_days` refuses with a 500 (Q-093) | 1 probe (avoided) |

## Edge cases of the domain, and what the app does

| Case | What the app does |
|---|---|
| A night shift runs past midnight | it belongs to the day it starts on: the night of the 24th is 19:00 on the 24th to 07:00 on the 25th, in leave, in the grid and in the calendar |
| Clocks change during a night | the calendar shows the night's own length (13 h on 24 Oct 2026, 11 h on 28 Mar 2026), from `date.at` in the factory's zone |
| Days before the rotation's first day | `mod()`, not `%`, so the crews come out right whichever side of EPOCH a day is |
| "Today" | the factory's (`date.today(FACTORY_TZ)`), on the server and in the page, not the viewer's machine's or UTC's |
| Leave over rest days | only working shifts count; a request of rest days only is refused ("nothing to take off") |
| Leave over the new year | each year's shifts count against that year's 14 (a request from 10 Dec to 9 Jan was refused for 2026's part) |
| A crew transfer inside a leave | the shifts are counted with the crew of each day; who decides is the supervisor of the line on the leave's first day |
| A transfer between crews | the roster runs on with no rest between them: E0001 moves from A to B on 11 Nov and works D D N N N N, six in a row. Not warned about yet |
| Annual leave after a transfer | the balance is worked out again from the crews held now, so it can differ from the `shifts` stored with an older request |
| Two requests over the same days | the second is refused, naming the first |
| Sick leave | from up to 7 days back to tomorrow; annual and unpaid from tomorrow on |
| A request over 31 days, the last day before the first, a day like 2026-02-30 or a date-time | refused with a reason, before any date function sees the text |
| Approval below the line's minimum | refused with the days and counts ("2026-11-02 D (4 of 5)") unless "approve anyway" is ticked and a reason given |
| Two approvals at once that together go below the minimum | one is approved, the other refused (serializable transaction) |
| Rejection | needs a reason, shown to the operator |
| Cancelling | a pending request any time; an approved one only before it starts |
| A supervisor deciding another line's request, an operator deciding, anyone deciding their own, no session | each refused, by the `server fn` (checked with direct calls) |
| Manpower ranges | next week is the next Monday to Sunday; months are calendar months; at most 62 days at once; a CSV of the same |

Not built: shift swaps, overtime, public holidays, supervisors' and managers' own leave,
transfers from the interface (the seed makes three), and notifications beyond the live pages.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| The rotation, DST nights, `date.format`, `date.parts`, `group_by` keys, negative `%` and indexes, `date.to_zone` and `date.date_in` at the hour that happens twice, lowercase zone names | the same on the server and in the browser, value for value |
| A component that reads `env("PUBLIC_FACTORY_TZ", "Europe/Berlin")` with a default | the default reaches the browser's bundle |
| `apply_leave` called with `kind: "vacation"`, a number for a day, extra fields, `note: null` | 400 naming the field; extra fields left out; `null` takes the default |
| Two approvals sent at once (serializable, SQLite) | one approved, the other refused with the shortfall |
| `publish` inside `db.transaction` | sent after the commit; never sent when the transaction rolled back |
| A month grid of 24 lines × 30 days × 2 shifts (1,440 cells) after one decision | a 421-byte patch, not the 133 KB month |
| Switching the grid from week (336 cells) to month (1,440) | about 100 ms in the browser; the loader takes 15 to 25 ms for a month, 27 to 32 ms for two |
| A request arriving and another leaving the supervisor's list while they type in a third | its text and focus kept (keyed `Decision` components) |
| Markup in an operator's note (`Family visit <b>abroad</b>`) | shown as text |
| Three roles' pages (operator, supervisor, manager) | taken up as the server sent them: nothing made again, no errors caught |
| One live loader followed by two components on a page, in two tabs | one run, answered to all four |
| An unknown zone (`Mars/Base`) in `date.today` | a clear runtime error naming it |
