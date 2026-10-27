# Validation

A form should say what is wrong while someone types, and the server must refuse what is
wrong whoever sends it. In Quillo both are the same code. A component's code, and the
functions and types it uses, is compiled to JavaScript, so the rules you write once give
hints in the browser and decide on the server, and the two never disagree.

The pieces are a **record** for the shape of the data, **plain functions** for the rules
the shape cannot say, a **component** that runs both as you type, and a **server fn** that
runs them again and has the last word. Everything on this page is one program; put the
parts in one file to try it.

### The shape: a record

A `type` says what a sign-up is: which fields, of which types, which choices, and what a
missing field defaults to.

```quillo
type Signup = {
    name: str,
    email: str,
    plan: "free" | "pro" = "free",
    seats: int = 1,
}
```

`Signup(fields)` makes one, or gives an error value saying what is wrong:
`` Signup: `plan` must be "free" | "pro", got str "gold" ``. It is the same function in
the browser and on the server, with the same message, and it keeps only the fields
`Signup` has. See [Records](/docs/language-tour/#records) for everything a record checks.

### The rules: plain functions

What a type cannot say goes in functions. Each rule gives a list of problems, a field and
a message, so rules add up with `+` and an empty list means all is well:

```quillo
fn problem(field: str, message: str) = [{ field, message }]
fn when(cond: bool, problems: list) = cond ? problems : []

fn is_email(s: str) {
    let parts = split(s, "@")
    len(parts) == 2 && parts[0] != "" && contains(parts[1], ".") && !contains(s, " ")
}

fn check_signup(s: map) =
    when(trim(s.name ?? "") == "", problem("name", "Your name, please"))
    + when(!is_email(s.email ?? ""), problem("email", "Not an email address"))
    + when(s.plan == "pro" && (s.seats ?? 1) < 2, problem("seats", "Pro is for teams of 2 or more"))
```

They only compute: they read the record and nothing else. That is what lets them run in
the browser (see [Keeping rules pure](#keeping-rules-pure)).

### In the browser: hints as you type

The form keeps what is typed in `state`, makes the record from it, and runs the rules on
it. Because a `let` follows the state it reads, `draft` and `problems` are worked out
again on every keystroke, and each hint updates on its own.

```quillo
fn message_for(problems: list, field: str) = (find(problems, (p) => p.field == field) ?? {}).message

component SignupForm() {
    state name = ""
    state email = ""
    state plan = "free"
    state seats = "1"
    state touched = false                   // no hints before the first try
    state sent = null                       // the draft the server last answered about
    state from_server = []
    state done = null

    let draft = Signup({ name, email, plan, seats: int(seats) ?? 0 })   // the record, in the browser
    let mine = is_err(draft) ? problem("", draft.error) : check_signup(draft)
    let problems = mine + (sent == draft ? from_server : [])          // the server's, while they apply
    let hint = (field) => touched ? message_for(problems, field) : null

    <form.signup @submit={() => {
        touched = true
        if len(mine) == 0 {
            let r = sign_up(draft)
            sent = draft
            if is_err(r) { from_server = r.error } else { done = r }
        }
    }}>
        <label>Name <input name="name" bind:value={name}/></label>
        <p.hint>{hint("name")}</p>
        <label>Email <input name="email" bind:value={email}/></label>
        <p.hint>{hint("email")}</p>
        <label>Plan <select name="plan" bind:value={plan}><option>free</option><option>pro</option></select></label>
        <label>Seats <input name="seats" type="number" bind:value={seats}/></label>
        <p.hint>{hint("seats")}</p>
        <p.hint>{hint("")}</p>
        <button>Sign up</button>
        {done && <p>Welcome, {done.name}.</p>}
    </form>
}

route GET / = <html><body><SignupForm/></body></html>
```

- **Hints wait for the first try.** `touched` keeps an empty form from opening with a
  list of complaints; after the first submit, hints follow every keystroke.
- **Nothing is sent that the browser already knows is wrong.** The submit calls the
  server only when the browser's own rules pass.
- **The server's answers stay as long as they apply.** A problem only the server can see
  (the address is taken) is kept with the draft it was about: `sent == draft` compares
  them by value, so typing a new address clears it, and typing the old one back shows it
  again.
- **Inputs bind to plain variables.** `bind:value` needs a variable of the component, not
  a field of a map, which is why each input has its own `state`. Do not put an `@input`
  handler on an input that has `bind:value`: it currently takes the binding's place, and
  the variable stops following the input. Work out what depends on the input with `let`,
  as `draft` is here.

### On the server: the same rules decide

The browser's hints are a courtesy: anyone can call the server without the page. The
`server fn` checks the shape, runs the same rules, then checks what only the server can
know.

```quillo
server fn sign_up(input: Signup) -> Signup ! list with db {
    let s = unwrap(Signup(input))           // only Signup's fields: anything else sent is dropped
    let problems = check_signup(s)
    if len(problems) > 0 { return err(problems) }
    if db.one("select 1 from accounts where email = ?", s.email) != null {
        return err(problem("email", "That address already has an account"))
    }
    db.exec("insert into accounts (email, name, plan) values (?, ?, ?)", s.email, s.name, s.plan)
    s
}
```

- **The shape is checked on the way in.** A parameter typed `Signup` is checked before
  the function runs; a call that does not fit is answered 400 with the reason, and the
  function never sees it.
- **Rebuild the record before using it.** A typed parameter checks the fields `Signup`
  has but keeps any others the caller added. `Signup(input)` keeps only its own fields,
  so an `is_admin: true` sent by a script goes no further. `input` has already passed the
  check, so making it again cannot fail: `unwrap` says so.
- **Errors come back in the shape the form reads.** `! list` declares that errors are a
  list of problems, so the component shows the server's under the right field, exactly as
  it shows its own. `quillo check` holds the function to it: a `?` that would pass on an
  error of another kind is reported before the program runs.

### What the server answers

The page's call is `POST /_quillo/rpc/sign_up`. This is what callers that skip the page
get back:

| Sent | Answer |
|---|---|
| a sign-up without `email` | 400: `` argument `input` of `sign_up` must be Signup, got map (`email` is missing) `` |
| `plan: "gold"` | 400: `` … (`plan` must be "free" \| "pro", got str "gold") `` |
| every field, `email: "nope"` | 200: `{"err": [{"field": "email", "message": "Not an email address"}]}` |
| every field, plus `is_admin: true` | 200: the sign-up, without `is_admin` |

A typed parameter needs every field, including those with defaults: `Signup(...)` puts
the defaults in, which is why the component sends the record it made and not the raw
form. A client of your own should send every field too.

### Keeping rules pure

Rules run where the component runs, so they can use what the browser has: text, numbers,
lists, maps and the standard library. A rule that reaches for the database, `http`,
`env`, files or the request cannot be compiled for the browser, and the program refuses
to start, naming it:

```
quillo: app.qwe:1: `taken` (compiled for the browser): `db` is not available in the browser; use it from a `server fn`
```

Checks like that belong in the `server fn`, after the shared rules, as the
"already has an account" check is above.

The clock and `random()` do work in the browser, but a rule that reads them can answer
differently on each side: a sign-up valid in the browser at 23:59 may be refused on the
server at 00:00. Pass such things in instead (today's date as an argument) so both sides
judge the same input the same way.

### Warnings as well as errors

Some problems are advice, not a refusal. A `level` on the problem says so, and only the
others block:

```quillo
fn warning(field: str, message: str) = [{ field, message, level: "warning" }]
fn errors(problems: list) = problems | filter((p) => p.level != "warning")

fn check_email(s: map) = when(ends_with(s.email ?? "", ".con"), warning("email", "Did you mean .com?"))
```

Show every problem as a hint, and decide on `errors(problems)`, in the browser and on the
server alike.

### A larger example

The [CRM](https://github.com/quillolang/quillo/tree/main/apps/crm) in the repository is
built this way. Each plugin brings rules for the kinds of records it cares about
(contacts, deals, products, campaigns, public forms); `validate(kind, record, ctx)` in
`lib/rules.qwe` runs those of the plugins a workspace has on. Its editors call it as you
type, and the `server fn`s that save call it again with a context built from the
database, so a rule written once checks a contact in the editor, in a CSV import and in a
public sign-up form.
