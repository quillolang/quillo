# Security

## Reporting a vulnerability

Please report vulnerabilities privately, through GitHub: on
[the repository's Security tab](https://github.com/wisp-one/quillo/security), choose
**Report a vulnerability**. Do not open a public issue.

Say what you found, the version or commit, and how to reproduce it (a small `.qwe`
program or a request is ideal). If you know what it gives an attacker, say that too.

## What happens then

- Within 3 working days: an answer saying the report was received.
- Within 10 working days: whether it is a vulnerability, how severe, and when a fix is
  expected.
- Critical and high findings are fixed first, before other work. The fix is released,
  then the advisory is published, crediting the reporter unless they prefer not to be
  named.

## Supported versions

Quillo is before 1.0: fixes go into the latest release only.

## Scope

The `quillo` runtime and server (`src/`), the browser runtime it serves
(`src/client/runtime.js`) and the editor extension (`editors/vscode`). Sandboxed scripts (`run_script`, `check_program`) are meant to be safe for
code written by a program's users, so a way out of them is in scope.

## What a visitor sees of a failure

An error a builtin made says `internal error` to a visitor, wherever it would reach one, and
its text goes to the log; `--watch` shows it, for development. Text a program sends is its
own: `err(...)`, and `str(r)` of an error, go out as they are.

## How dependencies are checked

`cargo deny` checks every dependency for known vulnerabilities, licences and sources in
CI (`deny.toml`), and Dependabot proposes updates weekly.
