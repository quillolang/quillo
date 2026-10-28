# Feedback from building a bank in Quillo

| | |
|---|---|
| App | [bank/](../../app/bank), "Quillo Bank": 2,095 lines of Quillo in 16 files, plus 144 of SQL and 230 of CSS. Five services, each a program with its own database (customers, accounts: a double-entry ledger, payments, risk, messages), called in chains through a gateway: a transfer goes gateway → payments → risk → accounts → messages. A phone app for customers (balances, statements, transfers checked as they are typed, messages), a dashboard for staff (figures, transfers held for a supervisor, desk sessions, activity, ledger health), and desk sessions: a clerk fills in a form at the counter while the client follows it live on their phone, approves each part, and chooses their PIN where the clerk cannot see it. |
| Quillo | 0.5.0 (5d2e21e) |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; probes of the risky parts before building; headless Chrome with up to three people signed in at once, each in a browser of their own (`app/bank/tests/*.mjs`: 55 checks); hostile calls to every server fn as each kind of visitor (`app/bank/tests/hostile.py`: 49); a service killed in the middle of a desk session and of a transfer (`app/bank/tests/outage.py`: 10); 1,000 transfers through the whole chain (`app/bank/tests/scale.py`); all of it in one process and with `--split`; `feedback/tools/mismatches.mjs` on every page, as a client and as staff |

What worked well: services. Five programs, each with its own database, called in chains
four deep, worked the first time and gave the same answers in one process and with
`--split`, with nothing in the code changed: all 104 browser and hostile checks pass both
ways. An error value raised three services down (`err("Insufficient funds")` in the
ledger) reaches the page unchanged through every `?`. Calls are fast: a transfer through
the whole chain takes 0.8 ms in one process and 1.3 ms split (eight HTTP calls, each
signed), 1,189 and 764 transfers a second one at a time. A service restarted by hand with
the same `QUILLO_SERVICE_SECRET` is trusted again, and a `later` call made while a service
was down was delivered 7 s after it came back, once. `db.transaction` rolled back on an
error value returned after a write, and 20 transfers at once from an account that covers
10 moved exactly 10. `money` gave the same text on the server and in the browser, and
`quillo check`'s warnings caught four names of mine that would have hidden builtins.

What it cost: live data across services does not work by itself (Q-038, Q-039), so the
gateway announces every change itself; and the most common way to show a part of a page
in JSX, `cond && <x>`, threw away the clerk's typing (Q-041) until it was found by tracing
which elements were made again.

## Seen again

| ID | Where it bit | Attempts |
|---|---|---|
| Q-027 | No date module: "28 Oct 2026" and the age check (18 or older) need lib/format.qwe's own calendar arithmetic (days to year, month, day), 25 lines | – |
| Q-028 | Each keystroke of the clerk's sends the whole desk session again to both screens, and runs `load_desk` (and its service calls) once for each. Fine at the counter's speed: two screens, a few KB | – |
| Q-008 | "if the code was written for a newer Quillo, that would explain it" after `route GET / with time { ... }`, my mistake in a repro. The note is gentler now, but still about versions | 1 |
| Q-015 | Avoided: about to name a component's parameter `key`, renamed to `field` before checking | 0 |

## New issues

### Q-041. `cond && <markup>` makes its elements again whenever anything it reads changes: an input in it loses its focus and what was typed

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the browser test (typing "Grace" gave "Gr"); then marking the input element and watching it disappear | 3 |

The clerk's form sat in `{desk.status == "open" && <div>...</div>}`. Each keystroke saves
the field and announces the desk, and the live reload that follows made every element
inside the `&&` again: the input being typed in was replaced, focus fell to `<body>`, and
the rest of the word went nowhere. The clerk typed "Grace" and the form said "Gr".

```quillo
component Form() {
    state data = { other: 0 }
    <div>
        <button id="bump" @click={() => { data = set(data, "other", data.other + 1) }}>bump</button>
        <div>{data.other >= 0 ? <div><input id="t"/> {data.other}</div> : <p>never</p>}</div>  // kept
        <div>{data.other >= 0 && <div><input id="a"/> {data.other}</div>}</div>                  // made again
        <div>{if data.other >= 0 { <div><input id="i"/> {data.other}</div> } else { null }}</div> // kept
    </div>
}
```

The guide says "`if`, `? :` and `match` in markup make a branch again only when another one
is chosen": `&&` is left out, and that is the form people coming from JSX write most. It
also explains less visible costs: every `{x && <b>...</b>}` in the bank's pages was
rebuilt on each live update. The first guess (a reload echoing an older value over newer
typing) was wrong: the field already kept its own draft. Only marking the input element
(`el.__mine = true`) and finding it gone showed that the element itself was replaced.
Repro: [feedback/repros/Q-041-and-in-markup-remakes-its-elements.qwe](../repros/Q-041-and-in-markup-remakes-its-elements.qwe).

**Workaround in the bank:** every piece of markup holding inputs or components is chosen with
`cond ? <x> : null` (ui/desk.qwe, ui/staff.qwe, with a comment saying why).

**Suggestion:** compile `cond && <markup>` as `cond ? <markup> : <nothing>`, which keeps its
elements while `cond` stays true (and `||`, `??` the same). Failing that, say so in the guide,
next to `? :`.

### Q-038. `publish()` in a service reaches the app's pages in one process, and silently nobody once `--split`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe in headless Chrome, both ways | 1 |

The ledger service announces its own changes with `publish("acct:<id>")`, and the
gateway's live loader watches that topic. In one process, a transfer updates every open
page. With `--split`, the ledger runs in a process of its own, its `publish` reaches no page,
and every balance stays as it was. Nothing says so, in either process's log.

| Who announces the change | One process | `--split` |
|---|---|---|
| nobody (the loader only calls the service) | page frozen (Q-039) | page frozen |
| the service, `publish(...)` | pages update | **pages frozen** |
| the gateway, after the call returns | pages update | pages update |

The guide says a program "behaves the same whichever way it runs", and this is where it
does not: an app tested in one process (the default) breaks when it is deployed split.
Repro: [feedback/repros/Q-038-publish-in-a-service-lost-when-split/](../repros/Q-038-publish-in-a-service-lost-when-split)
(with `--split`; without it, the same page shows the new balance).

**Workaround in the bank:** the gateway watches topics of its own (`cust:<id>`, `desk:<code>`,
`staff`) and publishes them itself after each service call that changes something
(lib/bank.qwe `cust_changed`, lib/desk.qwe `desk_changed`). A change a service makes alone
(a `later` call delivered, as a held message is after an outage) shows on the next load.

**Suggestion:** carry a service's `publish` back to its caller (with the answer of the call
that made it, or on the connection the services already share), so the topic is announced
where the pages are. Failing that, say in the guide that a service's `publish` reaches only
pages of its own process, and warn when `--split` starts a service that publishes.

### Q-039. A live loader that gets its data from a service follows only the service's name: the page never updates, without a word

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a probe in headless Chrome; the page's `data-l` | 1 |

```quillo
service ledger = "ledger.qwe"
server fn load() -> int with http = ledger.get() ?? -1

component Balance() {
    state bal = live(load)      // follows ["var:ledger"]: the variable holding the service
    ...
}
```

What a loader reads is recorded: here only the global `ledger`, which never changes. Writes
in the service are announced in the service, not to this program, so `bal` is its first
value for good, even on the page that made the change. In a program made of services this
is every loader. Repro:
[feedback/repros/Q-039-live-loader-calling-a-service-follows-nothing/](../repros/Q-039-live-loader-calling-a-service-follows-nothing).

**Workaround:** as for Q-038: `watch` a topic of the gateway's own in each loader, and
`unwatched(...)` around the service calls.

**Suggestion:** have `quillo check` warn about a `live` loader whose only topics are services
(and other values that never change): "this loader follows nothing that changes: `watch` a
topic and `publish` it after the calls that change it". Or follow a service's own topics
across the call, which would answer Q-038 too.

### Q-040. A `service` declared before a `use` of a file the service also uses: the program does not start

| Kind | Detected by | Attempts |
|---|---|---|
| loud (at start, not in `quillo check`) | starting the app | 2 |

```
quillo: bank/main.qwe:76: <DeskClerk> (compiled for the browser): unknown name `iban_pretty`
```

`lib/iban.qwe` is used by the gateway (its pages check account numbers as they are typed)
and by the accounts service (it makes them). With the `service` lines first, the browser
code of the gateway's components lacks the file's functions and the program stops;
with the `use` lines first, it runs. `quillo check` passes both. The guide says "the order
of `use` lines does not matter". A minimal case (a service and a component sharing one
helper) was needed to see the order mattered at all. Repro:
[feedback/repros/Q-040-service-before-use-breaks-browser-code/](../repros/Q-040-service-before-use-breaks-browser-code).

**Workaround in the bank:** the `use` lines come first in main.qwe, with a comment.

**Suggestion:** a file a service and its caller both use belongs to each program: give the
caller's browser code its functions whichever is loaded first (it looks as if the file is
marked as loaded once, process-wide).

### Q-044. `quillo check` lets `n / 4` through where a builtin takes an int: an int only when it divides evenly

| Kind | Detected by | Attempts |
|---|---|---|
| loud (at run time, for some data) | running a test script | 1 |

```quillo
fn groups(s: str) -> list = range(0, (len(s) + 3) / 4)   // ok in quillo check
print(groups("abcdefgh"))                                 // [0, 1]... or, for 9 characters:
// `range` expects an int as argument 2, got float
```

`/` is exact in Quillo: `6 / 3` is the int 2, but `7 / 2` is the float 3.5. `range`,
`slice`, `take` and the like take ints, so a call such as the one above works for some
lengths and stops for others: it can pass every test and fail in production. The checker
types `int / int` as "int or float", which is right, and lets it through where an int is
taken. (A first draft of this report said `int / int` is always a float; it is not.) Repro:
[feedback/repros/Q-044-int-divided-passed-as-int.qwe](../repros/Q-044-int-divided-passed-as-int.qwe).

**Suggestion:** warn where a `/` is passed straight to a parameter that takes only an int
("`/` gives an int only when it divides evenly: `floor(x / 4)`").

### Q-043. A program with a service logs `POST /_quillo/later -> 200` about once a second, idle

| Kind | Detected by | Attempts |
|---|---|---|
| loud (noise) | reading the log | – |

The sender of `later` calls asks the program itself, over HTTP, every second, and the
request log shows each one: 6 lines in 6 idle seconds, about 86,000 a day, with or without
any `later` call made. A program without a `service` has none. It buries the requests
people make. Repro:
[feedback/repros/Q-043-later-poll-logged-every-second/](../repros/Q-043-later-poll-logged-every-second).

**Suggestion:** leave Quillo's own `/_quillo/later` out of the request log (as the prefetch
refusals now are), or poll only when something is waiting.

### Q-042. `--split` does not start a service again when it dies

| Kind | Detected by | Attempts |
|---|---|---|
| limitation | killing the accounts service (`kill -9`) during the outage test | – |

The app stayed without its ledger until the whole app was restarted: every call to it an
error value (`unreachable at http://127.0.0.1:...; trying again`), `/health` 503. The bank
copes (pages say "your accounts cannot be shown right now", a desk session carries on when
the service is back), but nothing brings the service back: it had to be started again by
hand, at the same port, with the same secret. The guide does not promise more.

**Suggestion:** have `--split` start a service again when its process ends (with a pause
that grows, and a log line), as a process supervisor would; or say in the guide that it
does not, and that production should use one.

## Where the AI struggled

| What | Why it seemed right | What happened | Cost |
|---|---|---|---|
| `server fn get(id)`, `count()`, `find(id)` in the customers and payments services | a service's API: `customers.get(id)` | each hides a builtin inside the service; `quillo check` warned each time; three tries at names | 3 checks |
| A service file named `notify.qwe` | the obvious name for it | the service is named after its file, and `notify` is a builtin: it hid `notify` in every program using it (warned). Renamed `messages` | 1 check |
| `let view = ...`, `state view = ...` | the natural name for "what this side sees" | `view` is a keyword; the message now says so (Q-034's fix) | 1 check |
| `range(0, (len(c) + 3) / 4)` | integer division | `/` gives a float (Q-044); found when run | 1 run |
| `\\,` in a regular expression built in a template | escaping every separator to be safe | Quillo's patterns refuse needless escapes, with a clear message | 1 run |
| `"x" |> f` | another language's pipe | "the pipe is now a single `|`" | 1 run |
| A clerk's form in `{status == "open" && <div>}` | JSX's way to show a part | inputs made again on each update (Q-041); first blamed on an echo of older values, which the field already guarded against | 3 runs |
| Live balances read from the ledger service | `live(load)` works in one program | nothing updates (Q-039); a service's `publish` only in one process (Q-038) | 2 probes |
| `use` lines after `service` lines | "the order of `use` lines does not matter" | the program did not start (Q-040) | 2 runs |
| Amounts: "0.001" read as €1.00, "12,3,4" as €1,234 | thousands separators guessed from a count of marks | my bug, found by the hostile test: the parser now takes only well-formed amounts | 1 test |
| `.held` and `.done` as CSS classes for a queue row and a finished panel | short names | collided with the status pills' `held` and `done`: stretched pills, a test counting the wrong elements | 2 runs |
| `load_home` turning a failed accounts call into `[]` | `?? []` keeps the page working | during an outage it told Ada she had no accounts; it now says the ledger cannot be reached | 1 test |
| The Python test's cookie jar | ordinary HTTP client | the session cookie is `__Host-...` and `Secure`: browsers send it to `http://localhost`, Python's jar does not | 2 runs |

## Limitations

What is missing by design or not built yet, and what the bank does instead.

- **No live data across services out of the box** (Q-038, Q-039): the gateway publishes
  its own topics after each call that changes something.
- **No transactions across services.** Each service's `db.transaction` is all or nothing on
  its own database, so every movement of money is one entry in the ledger (both sides in
  one transaction, idempotent by `ref`), and the steps around it are recorded: a desk
  session remembers each step it finished (the customer made, the account opened) so a
  submit cut short by an outage carries on without making a second customer.
- **No rate limits** (removed in 205d7cd): wrong PINs are counted in the customers
  service's own table (five, then 15 minutes locked); messages and uploads count rows.
- **No date module** (Q-027): dates are computed from timestamps in lib/format.qwe.
- **`--split` keeps no service alive** (Q-042).
- **A `later` call's delivery reaches no page:** payments falls back to
  `messages.later.send` when the messages service is down, so a message is never lost, but
  it shows on the next load, not live.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| An error value from three services down (ledger → risk → payments → gateway) | arrives unchanged, in one process and split |
| `return err(...)` inside `db.transaction`, after a write | rolled back: no money moved |
| 20 transfers at once from an account that covers 10 | exactly 10 moved, the balance ended at 0, never below; the same split |
| Four supervisors approving the same held transfer at once | decided once (the transfer is claimed with `update ... where status = 'held'`) |
| A service named by three programs, `--split` | started once, shared; services before the gateway, so the demo data made at start found them |
| A service `use`-ing a shared file (`use "../../lib/iban.qwe"`) | works, in one process and split (but see Q-040 for the order) |
| A service's `server fn`s called from the page (`/_quillo/rpc/transfer`, `sign_in`, `deposit`...) | refused: only the gateway's fns are callable |
| Wrong argument types to a server fn (an id as text, an amount as a number, a map for the list) | 400, before any code runs |
| A desk session: the clerk changes a field after the client approved it | the approval stops counting, on both screens, live; an answer sent after the change is refused |
| A second phone, the clerk, or another customer answering for the client | refused; the PIN only on the phone that joined |
| The accounts service killed mid-submit, then started again by hand | the submit fails and says so; again: carries on, one customer, one deposit, the ledger adds up |
| The messages service killed during a transfer | the transfer goes through; the message and the email copy, sent `later`, arrive 7 s after it is back |
| `money` and `dec` in components (`money.format(..., "en-IE")`, `money.to_minor`) | the same as on the server; the transfer form checks amounts and IBANs in the browser with the server's own functions |
| Every page, as a client and as staff (`mismatches.mjs`) | taken up, never made again (`remade` 0), no errors |
| 1,000 transfers through the chain | all done; the ledger's rules (entries add up to zero, balances equal their postings) hold after, both ways |
