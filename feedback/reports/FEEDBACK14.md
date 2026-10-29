# Feedback from building a credit cooperative in Quillo

| | |
|---|---|
| App | [coop/](../../app/coop), "Quillo Coop": a credit cooperative of members who apply, are approved by a clerk and get a member number, savings and share capital; fixed deposits of 3 to 24 months; a loan designer in which the manager sets a product's terms and sees a sample schedule as they type, and which the board approves by vote; loan applications with co-makers, the clerk's review, the board's votes and the release into savings; salary deductions from employers, as a monthly batch the employer downloads and remits by CSV or by an API with its own key; web stores paid from savings or by salary installments; raffles drawn from a seed published before sales; events with tickets and check-in; a help desk; and a daily job that pays out matured deposits and posts savings interest. 3,289 lines of Quillo in 22 files, a migration, a stylesheet and six tests |
| Quillo | 0.6.0 (991e96c), the newest commit; all three findings open there |
| Model | Claude Opus 5.5 |
| Tested with | `quillo check`; four browser tests in headless Chrome: a loan's whole way from the application through co-maker, clerk, three directors and release (17 of 17), a member's pages (28 of 28), an employer's batch and remittance (19 of 19), the loan designer and the board's vote on a product (13 of 13); [tests/hostile.py](../../app/coop/tests/hostile.py), 73 direct calls to `server fn`s and routes as the wrong person, with bad data, and two clerks at once (73 of 73); [tests/jobs.py](../../app/coop/tests/jobs.py), the daily job (7 of 7); `tools/mismatches.mjs` on all 39 pages as each of the five roles (nothing made again, no errors) |

What worked well: money held as integer centavos and rates as basis points carried the
whole app. The loan schedule (diminishing balance or flat, rounded half up, the last
installment taking the remainder) is one pure `fn`, so the member's application form, the
manager's designer and the server's release work it out the same, centavo for centavo, with
nothing sent while typing. `db.transaction({ isolation: "serializable" })` rolled back
every half-done posting that returned an error value: a share capital withdrawal, savings
below the least kept, an order over the stock. Two clerks approving one applicant at the
same moment gave one member number and one sign-in. `idempotent({ user: ... }, fn)` made
an employer's API remittance safe to send twice: the replay was answered with
`Idempotent-Replayed: true` and posted nothing, the same key with another body got 422.
Records turned away a page's own price for an item, a negative quantity and extra fields
before any of the app's code ran. Live topics per member, per loan and per batch kept each
page up to date as other roles acted, with no polling.

What it cost: a `| flatten()` after a `for` block went inside it, and the salary batch
failed far away with "list has no field `member_id`" (Q-095). An `@input` beside a
`bind:value` silently dropped the typed amount, so the deposit form sent "" (Q-094). An
amount typed with too many digits crashed the `server fn` with a 500 instead of giving an
error value (Q-096). None of the three needed Quillo to change; each has a workaround in the
app, commented with its ID.

## Seen again

None of the known issues.

## New issues

### Q-095. `for x in xs => { ... } | flatten()` pipes each round's block, not the list

| Kind | Detected by | Attempts |
|---|---|---|
| silent | a runtime error far from the cause, in the payroll browser test | 3 |

The salary batch is built from every member of an employer: for each, a line for each loan
installment due, each store installment and the monthly savings. That is a list per member,
flattened:

```quillo
let pairs = for n in [1, 2] => {
    let tens = n * 10
    [n, tens]
} | flatten()
print(pairs)            // [[1, 10], [2, 20]], not [1, 10, 2, 20]
```

The `|` after the block's `}` belongs to the body, so each round flattens its own pair (a
list of numbers, unchanged) and the `for` gives a list of lists. In JavaScript
`xs.map((n) => { ... }).flat()` and Rust's `.map(|n| { ... }).flatten()` flatten the
result; the layout of the code says the same. `quillo check` says nothing. The batch was
stored, and `make_batch` failed when it read `line.member_id` from a list: "list has no
field `member_id`", a runtime error two functions away. I looked at the SQL and the records
before the `for`.

Repro: [Q-095](../repros/Q-095-pipe-after-for-block-is-inside.qwe).

**Workaround in the app:** parentheses round the `for` (`deductions_for` in lib/payroll.qwe):
`(for m in members => { ... }) | flatten()`.

**Suggestion:** end a `for`'s body at the block's `}` when it is a block, as `if` and
`while` do, so that a pipe after it takes the list; or have `quillo check` warn when a `|`
follows a `for`'s block body on the same line as the `}`, naming the parentheses.

### Q-094. `bind:value` and `@input` on one input: the handler takes the binding's place, silently

| Kind | Detected by | Attempts |
|---|---|---|
| silent | the member browser test (a fixed deposit of ₱"" refused) | 2 |

The fixed deposit form shows the interest a deposit would earn as the amount is typed. I
bound the amount and asked for the offer on each keystroke:

```quillo
component Deposit() {
    state amount = ""
    state seen = "nothing"
    <div>
        <input bind:value={amount} @input={() => { seen = `asked about ${amount}` }}/>
        <p>{amount}</p>          // stays "" whatever is typed
    </div>
}
```

The guide (Components) says it: an `@input` on the same input "takes the binding's place".
But the code reads like Svelte's `bind:value` with `on:input`, or Vue's `v-model` with
`@input`, where both run, and nothing warns. The input shows what was typed, the state stays
"", the offer said nothing, and the form sent an empty amount: "Enter an amount".

Repro: [Q-094](../repros/Q-094-input-handler-drops-bind-value.qwe) (a browser repro: the state
is "" after typing 20000).

**Workaround in the app:** `value={amount}` and `amount = e.target.value` in the handler
(`NewDepositForm` in ui/member.qwe).

**Suggestion:** `quillo check` warns about `bind:value` with `@input` (and `bind:checked`
with `@change`) on one element, naming the two ways the guide gives; or run the binding
first, then the handler, so the state has the value when the handler reads it.

### Q-096. `money.to_minor` of an amount past an int's range is a runtime error, not an error value

| Kind | Detected by | Attempts |
|---|---|---|
| loud | a hostile call with a 20-digit amount (a 500) | 1 |

Every amount in the app comes from a form as text and becomes centavos in one place,
`centavos(text)`, which tests `money.of(text, "PHP")` for an error value as the guide shows
(`money.of(form.amount, "EUR") ?? reply 400`). `money.of("99999999999999999999", "PHP")`
is money, and `money.to_minor` of it is a runtime error, "integer overflow", which `??`
cannot catch: the `server fn` answers 500. `money.of` also takes `"1e3"` as ₱1,000, which no
one types into a peso field on purpose.

```quillo
let m = money.of("99999999999999999999", "PHP")
print(type_of(m))           // money
print(money.to_minor(m))    // runtime error: integer overflow
```

Repro: [Q-096](../repros/Q-096-money-to-minor-overflow-is-not-an-error-value.qwe).

**Workaround in the app:** `centavos` first holds the text to a pattern of its own
(`^-?[0-9]{1,12}(\.[0-9]{1,2})?$`, after taking out `,`, `₱` and spaces) and only then calls
`money.of` (lib/amounts.qwe).

**Suggestion:** `money.to_minor` gives an error value when the amount does not fit, or
`money.of` refuses an amount whose minor units would not fit; and `money.of` of text takes
only decimal notation, as a form's amount is written.

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `reduce(xs, (acc, x) => ..., 0)` | JavaScript's order | `quillo check`: the start value comes first, `reduce(xs, 0, f)` | 1 check |
| `break` in the loop paying installments | most languages have it | there is none; `if left > 0 { ... }` round the body | 1 check |
| `get(rows, 0)` on a list | `get` for both | `get` is for maps; `rows[0]`, which is `null` past the end | 1 check |
| A type `NewRaffle` and a component `NewRaffle` | the form and its record named alike | types and components share one namespace; renamed `RaffleForm`, `EventForm` | 1 check |
| The seed without the `env` effect | it read `COOP_DEMO` in a helper | `quillo check` named the effect | 1 check |
| `reduce` over a string's characters | as over a list | a `for` over `chars` instead | 1 check |
| `temp_password` drawing two random numbers for one slice | the start and the end of a slice | a password of the wrong length now and then; one draw | 1 test |
| The browser tests typing as soon as the page loaded | a page looked ready | components wake on hover, focus or a key, and the first events went to plain HTML; the tests focus the field first | 3 runs |
| The member's one-time password shown in the application's live row | the row had the data | the row left the live list once approved, with the password; it is kept in the parent's state | 1 run |
| `window.quillo` in the payroll test | the runtime as a global | absent until the bundle loads; the test calls `/_quillo/rpc/new_api_key` | 1 run |
| `bind:value` with `@input` | Svelte and Vue habit | Q-094 | 2 runs |
| `} \| flatten()` after a `for` block | `.map(...).flat()` habit | Q-095 | 3 runs |
| A flat loan's monthly as ₱1,750.00 in the test | rounded in my head | ₱1,749.99: the remainder is in the last installment | 1 run |
| Carla as her own co-maker in a test | the simplest case | refused first for her open loan (`max_active`); Hugo is used | 1 run |

## Edge cases of the domain, and what the app does

| Case | What the app does |
|---|---|
| Money | integer centavos everywhere, `money.format` in fil-PH (₱1,234.56); rates in basis points; amounts typed with `,`, `₱` or spaces taken, more than two decimals or twelve digits refused |
| "Today" and the month | Asia/Manila's (`PUBLIC_COOP_TZ`), on the server and in the page; the daily job is scheduled at 16:30 UTC, 00:30 in Manila |
| Joining | an application (names, birth date, employer and employee number, the monthly savings pledged) waits for a clerk; approved, the member gets a number (M-0001), a one-time password that must be changed, a savings and a share capital account; rejected, a reason. One employee number per employer |
| Savings | can not go below the least kept (₱500 by default); share capital never goes out, only in (from cash or savings) |
| Savings interest | on the balance at the month's end, at the rate set by the manager, posted once a month (a key per month in `settings`), by the job on the 1st or by the manager's button |
| Fixed deposits | 3, 6, 12 or 24 months at the rate of the term, at least ₱10,000, from savings; paid into savings with its interest on its day; taken out early, without interest |
| A loan product | amounts, terms, rate and method, fee and insurance at release, the most as a multiple of share capital and savings, months of membership, co-makers, open loans at once, salary deduction only, approvals needed; designed as a draft, put to the board, active once enough directors vote for it, back to draft when it can no longer get there |
| Applying for a loan | checked against the product (range, term, months of membership, multiple, open loans, salary deduction and an employer, co-makers who are active members and not the applicant); the schedule shown as the amount is typed |
| Co-makers | asked, they agree or decline from their own page; a clerk can not forward a loan until every one agrees |
| Review and the board | the clerk forwards, returns (with a note) or rejects (with a reason); each director votes once; approved at the product's number of votes, rejected when it can no longer get there |
| Release | by a clerk, after the board: a loan account, the fee and insurance kept, the net into savings, the installments due from the next month |
| Paying | at the counter, from savings, or by salary; oldest installment first, interest before principal; more than is owed refused |
| Salary deductions | a batch per employer and month: each member's installments due, store installments and savings pledged; the employer downloads a CSV, remits by CSV or by the API; each member's money pays loans, then the store, then savings, and what is over goes to savings; short and missing lines marked; numbers the coop does not know listed back |
| The employer's API | a key per employer, kept as its SHA-256, shown once; idempotent per key; a remittance replayed posts nothing |
| Stores | the server's prices, member prices, stock taken in the same transaction as the order; paid from savings, at pickup, or in up to 6 salary installments for members with an employer |
| Raffles | a seed's hash published when the raffle opens; drawn after it closes: ticket = SHA-256(seed:sold) mod sold + 1, which anyone can check; a most per member |
| Events | capacity, a price paid from savings, one booking per member, a ticket code checked in once |
| Help desk | a member opens a ticket; clerks answer; either closes it; the thread is live |
| Roles | member, clerk, director (bod), manager and employer's payroll staff each reach only their own pages and `server fn`s |

Not built: dividends and patronage refunds at the year's end, loan penalties for late
installments, restructuring a loan, members leaving with their share capital paid back,
email or SMS notices, and printed passbooks.

## Edge cases checked that behave well

| Case | Result |
|---|---|
| Two clerks approving one applicant at the same moment | one member number, one sign-in, one set of accounts |
| A share capital withdrawal, savings below the least kept, more than the stock | refused, and nothing moved (the transaction rolled back on the error value) |
| A price, an item of another store, a negative quantity, extra fields sent by a page | the server's price; refused; nothing ordered; left out |
| A clerk voting, a director releasing or taking cash, a member reading another's loan or statement, an employer's staff reading another employer's batch | each refused by the `server fn` or route (403 or an error value) |
| A co-maker reading the loan they answer for | allowed, and only that loan |
| A director voting twice | counted once |
| The API without a key, with a made-up key, replayed, replayed with another body | 401, 401, the first answer again, 422 |
| The loan schedule in the page and on the server | the same, centavo for centavo, for both methods |
| Five roles' 39 pages | taken up as the server sent them: nothing made again, no errors caught |
| The daily job run twice in a day | the second run pays out nothing and posts no interest |
