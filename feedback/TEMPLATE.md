# Feedback from building <what> in Quillo

<!--
Copy to feedback/reports/ as FEEDBACKn.md (the next number). Before writing,
read feedback/INDEX.md: an issue already there is "seen again", not new.
Delete these comments when done.
-->

| | |
|---|---|
| App | [<folder>/](<folder>): one line on what it does, and its size (lines, files) |
| Quillo | `quillo --version` output, e.g. 0.5.0 (896e758) |
| Model | the AI model that built it |
| Tested with | `quillo check`, headless Chrome (`feedback/tools/`), scale (how many rows) |

What worked well: <a short paragraph: what the language made easy, with numbers where
there are some>.

## Seen again

<!-- Known issues this app ran into, by ID. Add the app to "Seen" in INDEX.md too. -->

| ID | Where it bit | Attempts |
|---|---|---|
| Q-012 | `let num = …` in lib/x.qwe | 2 |

## New issues

<!--
Most severe first: silent before loud, loud before docs. One section each, with:
- the fields below
- what happened, and what was expected
- a repro of a few lines (also saved as feedback/repros/Q-NNN-name.qwe, see
  feedback/README.md), checked on the version above
- what it cost: attempts, and how it was finally found
- a fix or a suggestion
-->

### Q-NNN. <the problem, as a statement>

| Kind | Detected by | Attempts |
|---|---|---|
| silent / loud / perf / docs / limitation | `quillo check` / run-time error / browser / reading the runtime / scale test | n |

<what happened>

```quillo
<the repro>
```

**Suggestion:** <fix>

## Where the AI struggled

<!--
What was tried first and why it failed, in order, including mistakes that were
the AI's own and not Quillo's. This is the record of how a model meets the
language, which the issues alone do not show.
-->

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| `{ ...m, k: v }` | JavaScript habit; also in the guide | parse error, with the version note | 1 check |

## Limitations

<!-- Missing by design or not yet built, and the workaround the app uses. -->

## Edge cases checked that behave well

| Case | Result |
|---|---|
