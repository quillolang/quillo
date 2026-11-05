# Feedback from a sweep of edge cases ten times FEEDBACK49's size: HTTP, files, databases, components, server and browser, core values, concurrency, modules, dates and text, and the tools

| | |
|---|---|
| App | None: about 7,500 edge cases in ten areas, plus 2,091 plural-rule comparisons with `Intl.PluralRules`. Probe programs of 1 to 25 lines, driven by `curl`, raw sockets (Python), Node clients for live data and channels, headless Chrome ([feedback/tools/drive.mjs](../tools/drive.mjs)), Node running the real runtime.js, PostgreSQL 17.11 and a MariaDB 12.3 of the sweep's own |
| Quillo | `quillo 0.7.0 (c15588c, 2026-11-05)`, a release build of main, built for this sweep. Nothing else was building or changing main while it ran |
| Model | Claude (Opus 5.5), with eleven agents: one per area, and a follow-up for files, `srcdoc` and highlighting after the security agent stopped early |
| Tested with | `quillo check`, `quillo run`, `quillo repl`, the other commands, the language server over stdio. References: Node's `Intl`, Python's `zoneinfo`, CLDR, RFC 9110 and 6265, `ffprobe`. Throwaway databases (`f55_*`), dropped afterwards; the shared PostgreSQL and MariaDB were not touched |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each issue's repro is in its section; the agents' probe programs are in this session's scratch folder (listed at the end) and are not kept. The highest number in use was Q-689 (FEEDBACK54): numbers here start at Q-700, to leave room.

FEEDBACK49 asked whether the 97.9% coverage reached the failure side, and probed about 150 cases to see. This sweep asked what that one missed, and ran ten times more. A sweep of the same areas was started on daa8c35 on 2026-11-03 and stopped with its session; only its database part finished. Its 15 database findings were run again here: 13 are unchanged on 0.7.0 and 2 changed in their messages only, so they are reported below as new.

Every issue below was reproduced at least twice by the agent that found it, and the worst of each area once more for this report (marked "reproduced for this report"). Each was checked against `feedback/INDEX.md` (Q-001 to Q-689) and the two reports not yet in it, FEEDBACK52 and FEEDBACK54. Where an issue gets around a fix already made, it says which.

## The numbers

| Area | Cases | Behaved well | Issues |
|---|---|---|---|
| HTTP: files, ranges, compression, cookies, forms, uploads, answers | about 540 | about 495 | 15 (Q-712 to Q-726) and 3 under security |
| Output and file security (stopped early, after about 110) | about 110 | 105 | 2 |
| Files, paths, `srcdoc`, highlighting (the follow-up) | 576 | 570 | 6 |
| Databases: values in and out of SQLite, PostgreSQL and MariaDB, the query API | about 1,050 runs of about 430 cases | about 990 | 30 (Q-727 to Q-756) and 1 under security |
| Components and forms in Chrome | 205 | 190 | 15 (Q-757 to Q-771) |
| Server and browser: one expression, both sides | 1,945 | 1,673 the same | 216 differences, 53 known; 11 issues (Q-772 to Q-782) |
| Core values and builtins | about 700 | about 655 | 13 (Q-783 to Q-795) |
| Concurrency, live data, channels, jobs | 183 | 148 | 11 (Q-796 to Q-806) |
| Files of a program, plugins, native Rust | about 250 | about 220 | 16 (Q-807 to Q-822) |
| Dates, time zones, plurals, numbers, Markdown, images | about 1,720, plus 2,091 plural comparisons | about 1,600 | 12 (Q-823 to Q-834) |
| The command line, `.env`, the policy, the language server | about 280 | about 255 | 18 (Q-835 to Q-852) |

153 issues in all, Q-700 to Q-852; small ones of one kind share a number. 109 are silent (a wrong result, or nothing done, with no error), 30 loud, 6 perf and 8 docs; 13 are security. Two are regressions of fixes: Q-746 (from FEEDBACK44's `begin`/`commit` refusal) and the variants of FEEDBACK51's fixes in Q-700, Q-704, Q-705, Q-707 and Q-708.

What behaves well is long, and is at the end: HTTP framing and paths, the date arithmetic (142 DST cases equal to Python's), integer plurals in 41 locales, 1,673 server and browser results, top-level state under 400 concurrent writers, and most of the database round-trips.

## Fix first

1. **Q-700**: visitor JSON in a spread turns any element into a component and runs script. It gets around Q-667's fix.
2. **Q-702, Q-701, Q-703**: `files(".")` serves the open database with its password hashes, a symlink serves `.env`, and `check_program` hands `.env` back to whoever sends code.
3. **Q-709**: a 204 stream corrupts the next answer on the connection, which a proxy can give to another visitor.
4. **Q-708**: the guard against session settings leaking between requests (Q-581) has four ways round it.
5. **Q-796**: `end_session` inside `parallel` does nothing, so a logout leaves the visitor signed in.
6. **Q-728, Q-727, Q-729**: MySQL commits half a transaction, an error value is stored as data, and SQLite stores NULLs for values never given.
7. **Q-706**: a 256 KB request to an endpoint every server has holds it for 22 seconds, past `--max-compute`.
8. **Q-757 to Q-760**: a number input rewrites what is typed, a select shows the wrong option, reordered rows lose their input.
9. **Q-772, Q-773**: a note typed in a textarea makes its component re-made on every load, and the re-make shows the errors production hides.
10. **Q-807, Q-798**: a scheduled job runs on every copy when they start from different folders, and a nightly cron job's first night is skipped.
11. **Q-835, Q-836**: one bad byte in `.env` makes every setting null without a word, and `--watch` started on a broken file never recovers.

## Summary

| ID | Problem | Kind |
|---|---|---|
| **Security and files** | | |
| Q-700 | A spread of visitor JSON writes `data-qi`, `data-c`, `data-p` and `data-s`: the element becomes a component the visitor chose, and its `children` run as HTML | silent, security |
| Q-701 | `files()` serves `.env` and `.qwe` source through a symlink with a harmless name | silent, security |
| Q-702 | `files(".", path)` serves the SQLite database the program has open, and its `-wal` | silent, security |
| Q-703 | `check_program` on a visitor's code reads `.env` and `.git/config` and returns their contents in its diagnostics | silent, security |
| Q-704 | The refusal to write `.env` is case-sensitive: `.ENV` is written, then read as the settings at the next start | silent, security |
| Q-705 | `delete_file` refuses the open database but deletes the folder that holds it | silent |
| Q-706 | Highlighting is quadratic: a 256 KB body to `/_quillo/markdown` holds the server for 22 s, past `--max-compute` | perf, security |
| Q-707 | A `css` template in `<svg><style>` can add rules: the browser decodes `&#125` there | silent, security |
| Q-708 | PostgreSQL: the Q-581 guard misses `set_config` with parameters, inside `values (...)` and in an `update`, and the setting reaches other visitors | silent, security |
| Q-709 | `stream` and `events` with status 204, 205 or 304 still send their body: the next answer on the connection is corrupted | silent, security |
| Q-710 | `respond` sends a `null`, list or map header value as text, in place of the header Quillo would send (`nosniff`, `private`) | silent, security |
| Q-711 | In the browser, `highlight(code, "constructor")` or `"__proto__"` throws and shows nothing | loud |
| **HTTP and cookies** | | |
| Q-712 | `set_cookie` ignores options it does not know: `sameSite: "Strict"`, `httpOnly: false` and `maxAge` do nothing | silent, security |
| Q-713 | `__Host-` and `__Secure-` cookies go out breaking their prefix's rules, and under `--watch` without `Secure`: browsers drop them | silent |
| Q-714 | One cookie name set twice keeps only the last, whatever its path or domain | silent |
| Q-715 | `files()` with `cache` sends its 404 page as `immutable` for a year | silent |
| Q-716 | Parts after the 1,000th of a multipart body are dropped, and the answer is 200 | silent |
| Q-717 | A form whose content type is not in lower case reads as empty | silent |
| Q-718 | The `Cookie` header is read loosely: of two cookies with one name the last wins, and spaces and quotes are kept | silent |
| Q-719 | `redirect()` takes statuses that are not redirects, and writes `Location` unescaped | silent |
| Q-720 | Some multipart filenames from clients other than browsers are lost | silent |
| Q-721 | `files()` serves a file at its path with a `/` added | silent |
| Q-722 | `files()`'s 404 page is replaced by the plain one for conditional and range requests | silent |
| Q-723 | Ranges and conditionals depart from RFC 9110: a weak ETag passes a strong comparison, a POST gets 304, a huge range end gives 200 | silent |
| Q-724 | The uncompressed copy of a compressible answer has no `Vary: accept-encoding` | perf |
| Q-725 | A HEAD of a large file, or a 10-byte range of one, is answered 503 while the same client has 8 large sends running | perf |
| Q-726 | `set_cookie` sends a 5,000-byte cookie; `file()`'s type errors say a str is not a str, and some bad types go out with no Content-Type | loud |
| **Databases** | | |
| Q-727 | An error value given as a query parameter is stored in the row (on SQLite, its JSON in an integer column), and `check` says ok | silent |
| Q-728 | MySQL: a statement that commits by itself (`truncate`, `create table`) inside `db.transaction` commits everything before it | silent |
| Q-729 | SQLite: `db.exec` with `?` or `:name` and no values stores NULLs | silent |
| Q-730 | PostgreSQL: a `bit(n)` parameter is cut to `bit(1)` by the cast Quillo adds | silent |
| Q-731 | PostgreSQL: an int where the server cannot infer a type is sent as text, so `greatest(?, ?)` of 10 and 9 is "9" | silent |
| Q-732 | PostgreSQL json and jsonb: numbers past i64 come back as floats, and one value serde cannot read fails the whole query | silent, loud |
| Q-733 | PostgreSQL: JSON text written to a json or jsonb column is stored as one JSON string, even with `?::jsonb` | silent |
| Q-734 | PostgreSQL: a float out of range for `real` or `float8` is stored as Infinity or 0 | silent |
| Q-735 | SQLite: NaN is stored as NULL, and -0.0 as 0.0 | silent |
| Q-736 | MySQL: `db.open` of a database that does not exist creates it | silent |
| Q-737 | PostgreSQL: `?schema=` in a URL drops `?options=` | silent |
| Q-738 | `db.transaction` ignores misspelled option keys (`isolaton`, `retrys`) | silent |
| Q-739 | MySQL: `changes` counts rows changed, not rows matched, so "update, else insert" fails | silent |
| Q-740 | MySQL: a `last_id` at or past 2^63 is negative | silent |
| Q-741 | MariaDB: `BIT(n)` columns come back as raw bytes passed off as text | silent |
| Q-742 | PostgreSQL: a type Quillo has no reader for comes back as control characters or hex, depending on the value; the guide's list is stale | silent, docs |
| Q-743 | PostgreSQL: BC dates are read as text that cannot be written back | loud |
| Q-744 | PostgreSQL `real` and MariaDB `FLOAT` read back widened: 0.1 is 0.10000000149011612 | silent |
| Q-745 | PostgreSQL: after an `alter table`, kept prepared statements fail on every request until the server restarts | loud |
| Q-746 | `db.exec` refuses function, procedure and trigger bodies as SQL that "begins or ends a transaction" (since a9b56bb) | loud |
| Q-747 | A map of values whose keys the SQL does not use fails when the SQL has no parameters (SQLite, PostgreSQL) | loud |
| Q-748 | MySQL: a map or record as the only `?` value fails | loud |
| Q-749 | PostgreSQL: the `?` scanner misreads `?\|\|`, `@?`, `?1` and nested comments | loud |
| Q-750 | Read replicas: a `select` that writes (`nextval`) takes the replica out of use for 10 s | perf |
| Q-751 | MySQL: a value over `max_allowed_packet` fails as "Broken pipe" or "Packets out of sync" | loud |
| Q-752 | SQLite's `@name` and `$name` work only when the map's key carries the sigil; on MySQL `@x` is silently null | docs |
| Q-753 | `last_id` and `changes` mean different things per database, and `db.exec("")` gives a stale count | docs |
| Q-754 | The guide's column-list `update` fails on MySQL, and on PostgreSQL with one column | docs |
| Q-755 | The Q-629 warning is wrong for PostgreSQL arrays (`= any(?)`) | loud |
| Q-756 | Smaller: PostgreSQL array lower bounds, `jsonb[]` of lists, MySQL URLs with IPv6 hosts, `bigint unsigned` and `tinyint(1)` worse than documented, `sql_name` of non-ASCII stops the program | silent |
| **Components and forms** | | |
| Q-757 | A two-way number input rewrites what is being typed: `1.05` becomes `15` | silent |
| Q-758 | `<select value={x}>` shows the wrong option: in the server's page, when made in the browser, and when its options come later | silent |
| Q-759 | Rows keyed inside `? :` or `match` are not keyed: reordering them clears every input | silent |
| Q-760 | A text input or select bound to an int state makes it text: `qty + 1` shows "121" | silent |
| Q-761 | `<select multiple value={list}>` never shows a selection, and each change keeps only the newest option | silent |
| Q-762 | A component that has left the page follows the parent's new props: its cleanup and pending handlers act on the next record | silent |
| Q-763 | A component whose root is an SVG element is invisible inside `<svg>` | silent |
| Q-764 | A bare assignment statement in a component runs once, not as an effect | silent |
| Q-765 | A range input with a dynamic `max` has its `value` clamped to the default maximum | silent |
| Q-766 | Without `Element.moveBefore` (Safari), a focused keyed row that moves loses focus | silent |
| Q-767 | `e.form` leaves out the submit button that was pressed | silent |
| Q-768 | A form reset empties inputs that were made in the browser | silent |
| Q-769 | `<template>` in a component loses its content, with a mismatch on every load | silent |
| Q-770 | A child that sets the parent's state as it starts makes the server's page show two values | silent |
| Q-771 | `autofocus` on markup that appears later does nothing, and the guide does not say so | docs |
| **Server and browser** | | |
| Q-772 | A CR or NUL in text makes the browser re-make the component on every load: any note typed in a `<textarea>` | silent |
| Q-773 | Errors the server hides in production are shown in full once the browser re-makes the component | silent, security |
| Q-774 | Attributes are not compared when a component starts in the browser: a different value replaces the server's without a report | silent |
| Q-775 | A whole float is an int in the browser, and code does other things: `5.0 / 0` stops, an `int` parameter takes `2.0` | silent |
| Q-776 | Ints past 2^53 in the browser: math functions stop, `factorial(19)` overflows, `==` with an equal float is false | silent |
| Q-777 | The shortest digits of a float break ties the other way: 34% of floats in [2^50, 2^51) are written differently | silent |
| Q-778 | String functions differ in the browser: order by UTF-16 unit, `trim`'s set, `replace(s, "", x)` | silent |
| Q-779 | HTML values made text in the browser are `"<html>"`, and `json.stringify` writes them empty | silent |
| Q-780 | NaN is refused for ordering on the server and ordered in the browser | silent |
| Q-781 | `json.stringify` turns to exponent form at other sizes on each side (1e16 against 1e21) | silent |
| Q-782 | Smaller: `json.parse` takes more in the browser, `??` misses a stop in a field or index, non-ASCII map keys, `float("0x10")`, U+2028 in regexes, time zone names | silent |
| **Core values** | | |
| Q-783 | `sum` of text joins it after a "0": `sum([1, 2, "3"])` is "33" | silent |
| Q-784 | Error values silently become text: as map keys, in `re.replace` callbacks, in `url_encode` | silent |
| Q-785 | A pattern that binds one name twice matches anything: `[x, x]` matches `[1, 2]` | silent |
| Q-786 | Builtins ignore extra arguments when the program runs: `xs.sort((a, b) => b - a)` sorts ascending | silent |
| Q-787 | An int and a float are compared through f64: `9007199254740993 == 9007199254740992.0`, and `sort` can leave ints out of order | silent |
| Q-788 | `floor(a / b)` gives wrong digits past 2^53, though the guide calls it integer division | silent |
| Q-789 | `round(x, n)` ignores a negative `n` for floats, and rounds big ints through a float | silent |
| Q-790 | `pow(int, int)` with an exponent of 2^32 or more falls back to a float: `pow(-1, odd)` is 1.0 | silent |
| Q-791 | `unique` is quadratic: 20,000 ints take 1.4 s, where Python takes 0.4 ms | perf |
| Q-792 | `json.stringify` writes functions as strings | silent |
| Q-793 | `math.trunc`, `floor` and `ceil` refuse values between 9.2e18 and 2^63 | loud |
| Q-794 | A map literal with a key written twice gets no warning | loud |
| Q-795 | The smallest int cannot be written as a literal, and the error gives no hint | loud |
| **Concurrency, live data, jobs** | | |
| Q-796 | `set_cookie`, `set_session` and `end_session` inside `parallel` are dropped: a logout that does not log out | silent, security |
| Q-797 | A global changed in a transaction that rolls back keeps its value, but live pages are never told | silent |
| Q-798 | A cron job's first due time after a start is skipped: a server started each day before 03:00 never runs `"0 3 * * *"` | silent |
| Q-799 | Channel `join` and `leave` do not pair: a page that closes while joining is counted online for ever | silent |
| Q-800 | Server fn answers are applied in the order they finish: an older, slower search overwrites a newer one | silent, docs |
| Q-801 | Topic names are not checked: one with a comma never wakes, a trailing `:*` wakes everything under it, a long one gets 414 | silent |
| Q-802 | `sleep` holds a wait thread for its whole length: 65 open `/feed` streams make a 10 ms request take 0.5 to 0.9 s | perf |
| Q-803 | `every` and `schedule` called in a route answer 200 and never run; `check` passes what the start refuses | silent |
| Q-804 | One failing `parallel` function does not stop the others, though the guide says it does | docs |
| Q-805 | Two requests of one visitor writing the session at once: the last cookie wins, and the guide does not warn | docs |
| Q-806 | Over a WebSocket a burst drops channel messages even for a page reading at full speed, though the guide says "reliably" | docs |
| **Files of a program, plugins, native Rust** | | |
| Q-807 | A private function's tag depends on the working directory: copies started from different folders all run a `schedule` job | silent |
| Q-808 | A browser name taken from two scripts: the last wins in every file, with no warning | silent |
| Q-809 | NaN and ±inf reach Rust as null | silent |
| Q-810 | An error value reaches Rust as an ordinary map | silent |
| Q-811 | A raw identifier (`r#type`) in a Rust file hides every `pub fn` after it | silent |
| Q-812 | `x.rs` and `x.qwer` in one folder delete each other's library, so `run --no-build` fails after `build` | loud |
| Q-813 | Rust that `check` accepts but the glue cannot build (`Option<&str>`, `#[cfg]` on a `pub fn`) loses the whole file | loud |
| Q-814 | A `//` line before the `//!` header makes Quillo ignore the header | loud |
| Q-815 | Common Rust signatures are refused: `std::` paths, `&'static str`, a comment after a parameter | loud |
| Q-816 | A Rust alias is the program's, not the file's, and the fix the warning gives does not parse | silent |
| Q-817 | A main file run through a symlink looks for its used files beside the link | loud |
| Q-818 | A `fn` after a top-level `return` in a used file passes `check`, and the program does not start | loud |
| Q-819 | Q-523's fix covers only `import x.qwe use name`: a missing file taken whole, or a missing `.rs`, still cascades | loud |
| Q-820 | Upper-case extensions: `use Lib.QWE` looks for `Lib.QWE.qwe`, and `"Tools.RS"` is read as Quillo | loud |
| Q-821 | Types and `as`: an alias of a type is no type, and `use one as Point` replaces Point's constructor | silent |
| Q-822 | Smaller messages from native modules and imports | loud |
| **Dates, i18n, images, text** | | |
| Q-823 | Fractional plural counts pick the wrong CLDR form in about 20 locales: French 1.5 is "kilos" | silent |
| Q-824 | Two `image.convert` calls to one target at once share a temp file: one fails, and the file holds the other's image | silent |
| Q-825 | `image.convert` and `image.info` ignore EXIF orientation: phone photos come out on their side | silent |
| Q-826 | Transparent pixels turn black in JPEG output | silent |
| Q-827 | In the browser, `format_number` of a large whole float writes its binary digits | silent |
| Q-828 | `date.format` styles for regional locales are not CLDR's (26 of 225 cases) | silent |
| Q-829 | Server and browser disagree on which time zones exist (`+0530`, `Etc/Unknown`) | silent |
| Q-830 | A huge service `timeout` or `cooldown` panics as the program loads | loud |
| Q-831 | `markdown_page` takes its title from a `#` line inside a code block, and none from a setext heading | silent |
| Q-832 | A whole float `count` reads "1.0 item" on the server and "1 item" in the browser | silent |
| Q-833 | `Accept-Language` reads the `q` weight only in lower case | silent |
| Q-834 | The highlighter mis-colours multi-line strings and some quotes, and colours a language it does not know | docs |
| **The command line, `.env`, the language server** | | |
| Q-835 | A `.env` that is not UTF-8, or not readable, is skipped without a word: every `env()` is null | silent |
| Q-836 | `--watch` started while a used file is broken never reloads when that file is fixed | silent |
| Q-837 | `quillo fix` writes its import before a file's byte-order mark, and the file no longer parses | silent |
| Q-838 | `.env`: `KEY= # note` sets "# note", though the guide says ` #` ends a bare value | silent |
| Q-839 | `.env`: an unclosed quote takes the rest of the file, while saying the line was skipped | silent |
| Q-840 | A NUL in a `.env` value panics | loud |
| Q-841 | `--reload` with `--instances` never reloads, and nothing says so | silent |
| Q-842 | `--split --reload` never takes a change to a service's file, though the log says it reloaded | silent |
| Q-843 | `--split --port 0` tells every service the app is at port 0, and the channel relay quietly stops | silent |
| Q-844 | Language server: a project opened through a symlinked path gets no diagnostics, and unsaved text is ignored | silent |
| Q-845 | Language server: a `content-length` header in another case hangs that request and every one after it | loud |
| Q-846 | `quillo install --dir --force demo` installs into a folder named `--force` | silent |
| Q-847 | `quillo serve` parses its options its own way: no `--port=8080`, and `--with --watch` starts "--watch" | loud |
| Q-848 | The REPL fed from a pipe stops at a line that is not UTF-8, with exit 0 | silent |
| Q-849 | A policy file takes limits of 0 that the command line refuses: `max_body = 0` answers every POST 413 | loud |
| Q-850 | `quillo new` into an existing empty folder needs write permission on its parent | loud |
| Q-851 | `--help` and leading options: `run -h` errors without a main.qwe, `lsp --help` starts the server, `quillo --watch main.qwe` prints the whole usage | loud |
| Q-852 | Smaller: suggestions miss `serve`, `classes` and `fcgi`; `--watch` prints errors twice; a policy file's BOM; `--env-file` missing from help; `--tls-cert` twice; the language server's protocol errors; `repl` ignores arguments | loud |

Also seen again, with no new number: Q-629 (a list in `where id in (?)` matches nothing on SQLite and MySQL, with no error; `check` now warns for a literal list), Q-008 (the "written for a newer Quillo" note after `\x41`, `\u{}` and `\U0001F600`). Q-590 and Q-598, open from FEEDBACK49, were not hit.

## Security and files

### Q-700. A spread of visitor JSON writes `data-qi`, `data-c`, `data-p` and `data-s`: the element becomes a component the visitor chose, and its `children` run as HTML

**Kind:** silent, security. **Seen in:** Chrome, 2 runs; the server's HTML reproduced for this report. **Where:** `spread_attr_allowed` in [src/interp.rs](../../src/interp.rs) (line 739) and `spreadAttrOk` in [runtime.js](../../src/client/runtime.js); `boot()`, which trusts those attributes, and the step that makes a string `children` raw HTML.

FEEDBACK51 lists a spread of visitor JSON as safe: it drops `on*`, `bind:on*` and `srcdoc`. It keeps the four attributes `boot()` reads to wake a component. A visitor who sends them names a component, its props and its state, and a string `children` is inserted as HTML. No `raw()` is involved. It gets around Q-667, whose fix covered `raw`.

```quillo
component Card(children) {
    state open = false
    <div.card><button @click={() => { open = !open }}>toggle</button>{open ? children : "closed"}</div>
}
route GET / = <html><head><title>t</title></head><body><Card>hello</Card><div {...(json.parse(request.query.m ?? "{}") ?? {})}>profile</div></body></html>
```

`/?m=` with `{"data-qi":"","data-c":"Card","data-p":"{\"children\":\"<img src=x onerror=window.__pwn=1>\"}","data-s":"{\"open\":true}"}` URL-encoded. The server writes `<div data-qi="" data-c="Card" data-p="{&quot;children&quot;: &quot;&lt;img src=x onerror=window.__pwn=1&gt;&quot;}" data-s="{&quot;open&quot;: true}">`, and Chrome ends with `<quillo-raw><img src="x" onerror="window.__pwn=1"></quillo-raw>` and `window.__pwn` set. It needs the visitor's state to differ from the server's, so that the component is made again; without `open` it is still taken over, but the HTML is not inserted.

**Suggestion:** a spread never writes `data-qi`, `data-c`, `data-p`, `data-s` or any other name `boot()` trusts, on both sides. Better, Q-667's own idea: a nonce per answer on the islands the server wrote.

### Q-701. `files()` serves `.env` and `.qwe` source through a symlink with a harmless name

**Kind:** silent, security. **Seen in:** probe, reproduced for this report. **Where:** `hidden_path(rel)` in [src/builtins.rs](../../src/builtins.rs) (about line 1388) looks at the name asked for, not at where it leads.

```quillo
route GET /s/*p = files("public", p)
```

With `ln -s ../.env public/settings.txt` and `ln -s ../main.qwe public/notes.txt`: `/s/settings.txt` is `SECRET=1` with a 200, `/s/notes.txt` is the program's source, and `/s/.env` is a 404. Q-475's fix judges a link by where it leads when it leads out of the folder; the hidden and program-file checks do not.

**Suggestion:** apply the hidden-file and own-file checks to the link's target as well as to the name.

### Q-702. `files(".", path)` serves the SQLite database the program has open, and its `-wal`

**Kind:** silent, security. **Seen in:** probe, reproduced for this report. **Where:** `hidden_path` and `files` in [src/builtins.rs](../../src/builtins.rs) (about 1388 and 1441) do not consult `db::OPENED`, which `own_file` already uses when writing.

```quillo
db.open("app.db")
db.exec("create table if not exists users (email text, password_hash text)")
db.exec("insert into users values ('alice@example.com', 'HASHVALUE')")
route GET /*path = files(".", path)
```

`/app.db` (and `/APP.DB`) is a 200 of `application/octet-stream` starting "SQLite format 3"; `/app.db-wal` is a 200 holding the email and the hash three times each; `/main.qwe` is a 404. `quillo run index.html` itself suggests `route GET /*path = files(".", path)`, and `quillo serve .` in a program's folder serves `*.db` too.

**Suggestion:** 404 for the open databases and their `-wal`, `-shm` and `-journal`, in `files()` and `quillo serve`.

### Q-703. `check_program` on a visitor's code reads `.env` and `.git/config` and returns their contents in its diagnostics

**Kind:** silent, security. **Seen in:** probe. **Where:** `file_at` in [src/check.rs](../../src/check.rs) (about line 1915) and the link closure after it check only for plain path parts and `stays_in`.

The guide says the code given to `check_program` "may come from the program's users".

```quillo
// .env holds SECRET=APPENV
route GET /cc = str(check_program("probe.qwe", { "probe.qwe": request.query.code ?? "" }))
```

`/cc?code=use ".env"` answers `[{ file: ".env", line: 1, severity: "error", message: "unknown name `APPENV`" }, ... "cannot assign to `SECRET`" ...]`. `DB_PASSWORD=hunter2-secret` comes back as "unknown name `hunter2`" and "unknown name `secret`", and `use "site/.git/config"` as "found `gitconfig`".

**Suggestion:** confined checking refuses dot segments as `files()` does, and files that are not `.qwe`.

### Q-704. The refusal to write `.env` is case-sensitive: `.ENV` is written, then read as the settings at the next start

**Kind:** silent, security. **Seen in:** probe. **Where:** `own_file` in [src/builtins.rs](../../src/builtins.rs) (about line 1918): `name == ".env" || name.starts_with(".env.")`, and the `-journal` suffix test.

FEEDBACK51 says `own_file` holds case-insensitively on APFS. That is true only once a file named exactly `.env` exists.

```quillo
print(write_file(".env", "A=1"))                       // err("write_file: will not write .env: ...")
print(write_file(".ENV", "ADMIN_TOKEN=attacker"))      // null: written
print(write_file(".env", "ADMIN_TOKEN=attacker2"))     // null: the file is there now, as .ENV
// the next start: env("ADMIN_TOKEN") is "attacker2"
```

The same holds for an upload named `.ENV` and then `.env`, for `.ENV.LOCAL` through `write_file` and `.ENV.PROD` through `image.convert`, and, once, for `data/APP.DB-JOURNAL` next to the open database.

**Suggestion:** compare names in lower case (and after Unicode case folding) in `own_file`.

### Q-705. `delete_file` refuses the open database but deletes the folder that holds it

**Kind:** silent. **Seen in:** probe. **Where:** `own_file` in [src/builtins.rs](../../src/builtins.rs) (about 1921 to 1927) compares `target == db`.

```quillo
db.open("data/app.db")
db.exec("create table if not exists t (x int)")
print(delete_file("data/app.db"))   // err("... it is the database the program has open")
print(delete_file("data"))          // null: data/ is gone, with app.db, -wal and -shm
```

Loaded files are already protected "or a folder holding one"; the database is not. `"data/" + ""` does the same.

**Suggestion:** refuse when the open database lies under the target (`db.starts_with(target)`).

### Q-706. Highlighting is quadratic: a 256 KB body to `/_quillo/markdown` holds the server for 22 s, past `--max-compute`

**Kind:** perf, security. **Seen in:** probe; the scaling reproduced for this report. **Where:** `in_route_path` in [src/highlight.rs](../../src/highlight.rs) (about line 207, called at 264) splits the line from its start again for every `/*`. It is reached from Markdown fences ([src/builtins.rs](../../src/builtins.rs) about 2433) and `/_quillo/markdown` ([src/server.rs](../../src/server.rs) about 1264). runtime.js's `inRoutePath` has the same loop.

```sh
quillo run main.qwe --port P --max-compute 2s     # main.qwe: route GET / = "hello"
python3 -c 'import json;print(json.dumps(["```quillo\nroute GET "+"/*"*128000+"\n```",{}]))' > b.json
curl -X POST -H 'content-type: application/json' --data-binary @b.json http://127.0.0.1:P/_quillo/markdown
```

`200` after 22.77 s; a `GET /` sent a second later took 21.78 s. The log said "an instance has been computing for 5 s without letting go", and nothing ended it. `highlight("route GET " + repeat("/*", n), "quillo")` takes 0.09 s at n = 8,000, 0.35 s at 16,000, 1.41 s at 32,000 and 5.88 s at 64,000. The endpoint needs no route, so every Quillo server has it. `markdown()` and `markdown_page` of visitor text reach the same code.

**Suggestion:** track the route path once per line (linear time), on both sides; count highlighting against `--max-compute`; and limit `/_quillo/markdown`'s body.

### Q-707. A `css` template in `<svg><style>` can add rules: the browser decodes `&#125` there

**Kind:** silent, security. **Seen in:** Chrome, 2 runs. **Where:** the `f.style` branch of `render_contents` in [src/interp.rs](../../src/interp.rs) (about line 4542, `.replace('<', "\\3c ")`) and the `css` template's value escaping.

Q-668's fix escapes `{ } ; \` in `${}` values, and Q-646's turns `<` into `\3c `. Neither escapes `&`. In an HTML `<style>` that is fine, but inside `<svg>` the browser decodes character references in style text, and `&#125` needs no `;`.

```quillo
route GET /svgcss = <html><head><title>t</title></head><body><p #p>hi</p><svg width="10" height="10"><style>{css`circle { fill: ${request.query.q ?? "red"} }`}</style><circle r="5"/></svg></body></html>
```

`?q=` with `red&#125 #p&#123color:rgb(1,2,3)&#125` URL-encoded: the server writes it as given, and `getComputedStyle(#p).color` is `rgb(1, 2, 3)`. The same template in an HTML `<style>` leaves `#p` black.

**Suggestion:** escape `&` in `${}` values as `\26 `, or write an SVG or MathML `<style>`'s text with `&` as `\26 `.

### Q-708. PostgreSQL: the Q-581 guard misses `set_config` with parameters, inside `values (...)` and in an `update`, and the setting reaches other visitors

**Kind:** silent, security. **Seen in:** probe on PostgreSQL 17, 8 tries; the code read for this report. **Where:** `statement_changes_session` in [src/db.rs](../../src/db.rs) (about line 367) looks for `set_config` only in statements that start with `select` or `with`, and `set_config_lasts` (about 380) only takes a literal `false`.

Outside a transaction, `select set_config('app.a', '1', false)` is refused, as Q-581's fix intends. These are not:

```quillo
db.exec("select set_config('app.user_id', ?, ?)", "42", false)
db.exec("select set_config('app.user_id', '42', 'false')")
db.exec("values (set_config('app.user_id', '42', false))")
db.exec("update t set x = x where set_config('app.user_id', '42', false) is not null")
```

In a server, after `/login` ran the first form, `/who` (`current_setting('app.user_id', true)`) answered `{"uid":"42"}` on every fourth request: a row-level security setting from one visitor read by others.

**Suggestion:** look for `set_config(` in any statement, and refuse it unless its third argument is the literal `true` (or check the session state after the statement, as `DISCARD ALL` would show).

### Q-709. `stream` and `events` with status 204, 205 or 304 still send their body: the next answer on the connection is corrupted

**Kind:** silent, security. **Seen in:** probe, reproduced for this report. **Where:** [src/stream.rs](../../src/stream.rs) (about 111 to 114) takes any status from 200 to 599; [src/server.rs](../../src/server.rs) (about 2894 to 2900) runs the function after `write_stream_head` has written a head with no body ([src/httpd.rs](../../src/httpd.rs) about 2235).

```quillo
route GET /empty = stream((send) => { sleep(50); send("leftover") }, { status: 204 })
route GET /next = "next answer"
```

Two requests on one connection get `HTTP/1.1 204 No Content ... \r\n\r\n8\r\nleftover\r\n0\r\n\r\nHTTP/1.1 200 OK ...`. Python's `http.client` fails `/next` with `BadStatusLine 8`, and curl says "Invalid status line". Behind a proxy that keeps connections to the server, those bytes are the start of the next visitor's answer. `respond` already refuses 1xx and sends 204 and 304 without a body.

**Suggestion:** refuse 204, 205 and 304 (and 1xx) for `stream` and `events`, as `respond` refuses 1xx.

### Q-710. `respond` sends a `null`, list or map header value as text, in place of the header Quillo would send

**Kind:** silent, security. **Seen in:** probe. **Where:** [src/builtins.rs](../../src/builtins.rs) (about line 1664, `v.display()`).

```quillo
route GET /nul = respond(200, "x", { "x-content-type-options": null, "x-list": ["a", "b"] })
route GET /me = respond(200, `hi ${session().user ?? "anon"}`, { "cache-control": null })
```

The answers carry `x-content-type-options: null` (so no `nosniff`), `x-list: ["a", "b"]`, and on a signed-in page `cache-control: null` in place of `private`. `{ "set-cookie": ["a=1", "b=2"] }` sends one header, `set-cookie: ["a=1", "b=2"]`. `site_headers` documents `null` as "drop the header".

**Suggestion:** `null` drops the header (keeping the security headers Quillo adds unless the program names another value); a list sends one header per item for `set-cookie` and is an error elsewhere; a map is an error.

### Q-711. In the browser, `highlight(code, "constructor")` or `"__proto__"` throws and shows nothing

**Kind:** loud. **Seen in:** Chrome, 2 runs. **Where:** runtime.js about lines 1642 and 1662: `langs[String(name || "").toLowerCase()] || plain` finds the object's inherited properties.

A component showing `highlight(c, lang)` with `lang` from the query: `?l=constructor` logs `quillo: Cannot read properties of undefined (reading 'some')` and the `<pre>` stays empty; the server writes the escaped text. Any case of `constructor` or `__proto__` does it.

**Suggestion:** `Object.hasOwn(langs, key)`, or a map made with `Object.create(null)`.

## HTTP and cookies

### Q-712. `set_cookie` ignores options it does not know

**Kind:** silent, security. **Where:** `cookie_line` in [src/session.rs](../../src/session.rs) (about line 157) reads only the keys it knows.

```quillo
route GET / = { set_cookie("x", "v", { httpOnly: false, maxAge: 60, sameSite: "Strict" }); "ok" }
```

Sends `x=v; Path=/; HttpOnly; Secure; SameSite=Lax`: Express's spellings do nothing, so the cookie is `Lax` where `Strict` was asked for, and lasts the session. `quillo check` says ok.

**Suggestion:** refuse an unknown option and name the ones taken, as `file()` does since Q-331, and say "write it `same_site`" for the camelCase forms.

### Q-713. `__Host-` and `__Secure-` cookies go out breaking their prefix's rules, and under `--watch` without `Secure`

**Kind:** silent. **Where:** [src/session.rs](../../src/session.rs) (about line 205).

`set_cookie("__Host-id", "v", { path: "/app" })` sends `Path=/app`; with `domain`, or `secure: false`, it sends those too. `set_cookie("__Secure-t", "v", { secure: false })` sends no `Secure`. A plain `set_cookie("__Host-x", "v")` under `--watch` sends `__Host-x=v; Path=/; HttpOnly; SameSite=Lax`. Browsers drop all of these, so the cookie is never seen. `delete_cookie` already knows the prefixes need `Secure`.

**Suggestion:** an error for options a prefix forbids, and `Secure` (with `Path=/` for `__Host-`) forced for prefixed names, under `--watch` too.

### Q-714. One cookie name set twice keeps only the last, whatever its path or domain

**Kind:** silent. **Where:** `queue` in [src/session.rs](../../src/session.rs) (about 242 to 246) matches on the name only.

`delete_cookie("theme", { path: "/old" }); set_cookie("theme", "dark")` sends only `theme=dark; Path=/`: the old cookie at `/old` stays in the browser. `set_cookie("t", "1", { path: "/a" }); set_cookie("t", "2", { path: "/b" })` sends only `/b`. Browsers key cookies by name, domain and path.

**Suggestion:** replace a queued cookie only when its name, domain and path all match.

### Q-715. `files()` with `cache` sends its 404 page as `immutable` for a year

**Kind:** silent. **Where:** [src/builtins.rs](../../src/builtins.rs) (about 1421 to 1431) removes the ETag, Last-Modified and Accept-Ranges from the 404 page but keeps Cache-Control.

```quillo
route GET /build/*p = files("dist", p, { cache: "1y" })   // dist/404.html exists
```

`/build/app.2.js`, asked for before it is deployed, is `404` with `cache-control: public, max-age=31536000, immutable`, and stays a 404 in CDNs and browsers for a year.

**Suggestion:** `cache-control: no-cache` (or none) on the 404 page.

### Q-716. Parts after the 1,000th of a multipart body are dropped, and the answer is 200

**Kind:** silent. **Where:** [src/upload.rs](../../src/upload.rs) (about line 116, `for _ in 0..MAX_PARTS`).

```quillo
route POST /up { files: { f: [map] } } = { fields: len(request.form), files: len(f) }
```

999 text parts and then 5 files give `200 {"fields":999,"files":1}`: four files are gone. 1,500 fields keep 1,000.

**Suggestion:** 413 (or 400) past the limit, as the other limits answer.

### Q-717. A form whose content type is not in lower case reads as empty

**Kind:** silent. **Where:** [src/server.rs](../../src/server.rs) (about line 287): `ctype.contains(...)`, where `essence()` exists.

`curl -H 'Content-Type: Application/x-www-form-urlencoded' -d a=1 /form` gives `request.form` as `{}`. JSON and multipart are matched in any case. Media types are case-insensitive (RFC 9110).

**Suggestion:** compare the lower-cased essence.

### Q-718. The `Cookie` header is read loosely

**Kind:** silent. **Where:** [src/server.rs](../../src/server.rs) (about 264 to 267).

`Cookie: a=admin; a=root` gives `{"a":"root"}`. Browsers send the cookie with the longer path first, so the path-specific cookie loses; Express and Go keep the first. `a = 1` gives `{"a ":" 1"}`, and `a="q"` keeps its quotes.

**Suggestion:** keep the first of a name, trim spaces around `=`, and remove one pair of surrounding quotes (RFC 6265 `cookie-value`).

### Q-719. `redirect()` takes statuses that are not redirects, and writes `Location` unescaped

**Kind:** silent. `redirect("/x", 304)` sends `304 Not Modified` with a `Location`; 305 and 399 pass too. `redirect("/a b")` sends `location: /a b`, and `"/a\"><script>"` goes as written (no CR or LF, which are refused).

**Suggestion:** take 301, 302, 303, 307 and 308 only, and percent-encode what a URL may not hold.

### Q-720. Some multipart filenames from clients other than browsers are lost

**Kind:** silent. **Where:** [src/upload.rs](../../src/upload.rs) (about 64 to 75).

`filename="a\"b.txt"`, which Go's `mime/multipart` writes for a quote in a name, gives `filename: ""`. A part with only `filename*=UTF-8''...` (RFC 5987) becomes a text field in `request.form`.

**Suggestion:** read `\"` inside a quoted filename, and take `filename*` when there is no `filename`.

### Q-721. `files()` serves a file at its path with a `/` added

**Kind:** silent. **Where:** [src/builtins.rs](../../src/builtins.rs) (about line 1387, `trim_matches('/')`).

`/s/a.txt/` and `/s/about/` are 200 with the file: relative links in it then resolve under the wrong folder, and the page has two URLs.

**Suggestion:** 404 for a file asked for with a trailing slash (or 301 to the path without it).

### Q-722. `files()`'s 404 page is replaced by the plain one for conditional and range requests

**Kind:** silent. **Where:** [src/builtins.rs](../../src/builtins.rs) (about line 1422).

A missing path asked for with `Range: bytes=0-1`, `If-None-Match: *` or a future `If-Modified-Since` gets `not found` in place of the folder's `404.html`: the conditionals are applied to the 404 page itself, and its 206 or 304 is thrown away.

**Suggestion:** answer the 404 page without applying the request's conditionals or range to it.

### Q-723. Ranges and conditionals depart from RFC 9110

**Kind:** silent. **Where:** [src/statics.rs](../../src/statics.rs) (about 288 to 320) and [src/builtins.rs](../../src/builtins.rs) (about 1157 to 1179).

- `If-Match` and `If-Range` with the file's weak ETag match, giving 206 and 200; a weak tag never passes a strong comparison.
- A POST with a matching `If-None-Match` gets 304, where the RFC says 412.
- `bytes=0-99999999999999999999` overflows u64 and gives 200 with the whole file; it should be a 206 to the file's end.
- `Bytes=0-1` is ignored, though the unit is case-insensitive.
- `bytes=+2-4` is taken as a range.

### Q-724. The uncompressed copy of a compressible answer has no `Vary: accept-encoding`

**Kind:** perf. **Where:** [src/server.rs](../../src/server.rs) (about line 2708).

Only the gzipped answer carries `Vary`. A shared cache that first sees a request without `Accept-Encoding` keeps the uncompressed copy for everyone.

**Suggestion:** `Vary: accept-encoding` on every answer of a type that could have been compressed.

### Q-725. A HEAD of a large file, or a 10-byte range of one, is answered 503 while the same client has 8 large sends running

**Kind:** perf. With eight large file sends in progress from one client, a HEAD of a file over 1 MiB, or a 10-byte range of it, is refused with 503. Neither sends much, and a HEAD sends no body.

**Suggestion:** count against the per-client limit only what will be sent.

### Q-726. `set_cookie` sends a 5,000-byte cookie; `file()`'s type errors say a str is not a str, and some bad types go out with no Content-Type

**Kind:** loud.

- `set_cookie` with a 5,000-byte value sends a Set-Cookie line of 5,056 bytes, which browsers drop (over 4,096). `set_session` refuses at 3,900.
- `file("a.txt", { type: "text/plain\r\nx" })` and `{ type: "" }` say "`file`'s type is a str, got ...": it is a str. `{ type: "text/plain\u0001" }` answers 200 with no Content-Type, the header dropped as it is written and only logged ([src/httpd.rs](../../src/httpd.rs) about 2224); `stream(..., { type })` does the same, while `respond` gives an error.
- `set_cookie`'s path `"/a b"` or `"/a; Domain=x"` is told the "path starts with /".

**Suggestion:** a size check in `set_cookie` like `set_session`'s, and messages that say what is wrong with the value.

## Databases

Run on SQLite, PostgreSQL 17.11 and MariaDB 12.3.3. The ones marked "since daa8c35" were found by the stopped sweep and run again on 0.7.0.

### Q-727. An error value given as a query parameter is stored in the row, and `check` says ok

**Kind:** silent. **Since daa8c35;** reproduced for this report. **Where:** [src/db.rs](../../src/db.rs) (about line 728, `other => Sql::Text(other.to_json())`) and [src/pg.rs](../../src/pg.rs) (about 1095, `other => other.display()`).

```quillo
db.open(":memory:")
db.exec("create table people (name text, age integer)")
let age = int("4O")
print(db.exec("insert into people (name, age) values (?, ?)", "ann", age))   // { changes: 1, last_id: 1 }
print(db.query("select name, age, typeof(age) as t from people"))
// [{ name: "ann", age: "{\"error\":\"not an int: \\\"4O\\\"\"}", t: "text" }]
```

On PostgreSQL a text column gets `err("invalid JSON: ...")`; a function value is stored as `"<fn <fn>>"` on both. MySQL refuses it, as "a error cannot be stored in the database".

**Suggestion:** an error value as a parameter gives that error back (the guide's rule for builtins given an error), on all three; a function is refused.

### Q-728. MySQL: a statement that commits by itself inside `db.transaction` commits everything before it

**Kind:** silent. **Where:** nothing checks the server's in-transaction flag after a statement ([src/my.rs](../../src/my.rs) about line 218, [src/txn.rs](../../src/txn.rs)).

```quillo
c.transaction(() => {
    c.exec("truncate table rates")
    c.exec("insert into rates values (?, ?)", "EUR", 3)?
    c.exec("insert into rates values (?, ?)", "EUR", 4)?    // duplicate key
})
```

The transaction gives back the duplicate-key error, but the table now holds only `EUR 3`: the `truncate` committed, and the rows before it are gone. PostgreSQL and SQLite keep the table as it was. Nested transactions around a `create table` do the same: every row is kept, and both levels say they failed. The guide says so for migrations only (USERGUIDE.md line 6309).

**Suggestion:** after each statement in a MySQL transaction, check `SERVER_STATUS_IN_TRANS`; if the server committed, end the function with an error that names the statement. Or refuse statements that commit implicitly inside `db.transaction`.

### Q-729. SQLite: `db.exec` with `?` or `:name` and no values stores NULLs

**Kind:** silent. Reproduced for this report. **Where:** [src/db.rs](../../src/db.rs) (about line 1640): with no values, the SQL goes to `execute_batch`.

```quillo
db.open(":memory:")
db.exec("create table ub (name text, qty int)")
print(db.exec("insert into ub (name, qty) values (?, ?)"))   // { changes: 1, last_id: 1 }
print(db.query("select * from ub"))                          // [{ name: null, qty: null }]
```

PostgreSQL says "the query has 2 parameters, got 0", and MySQL gives a syntax error. The guide says a different count is an error.

**Suggestion:** count the placeholders on SQLite too, and refuse when values are missing.

### Q-730. PostgreSQL: a `bit(n)` parameter is cut to `bit(1)` by the cast Quillo adds

**Kind:** silent. **Where:** `casts_for` and `type_name` in [src/pg.rs](../../src/pg.rs) (about 736 and 1185) cast to the bare type name, and a bare `bit` is `bit(1)`.

```quillo
db.exec("create table perms (flags bit(8))")
db.exec("insert into perms values (?::bit(8))", "00000101")
print(db.one("select flags from perms"))        // { flags: "00000000" }
print(db.one("select ?::bit(4) as v", "1010"))  // { v: "1000" }
```

A plain `values (?)` is refused (22026).

**Suggestion:** cast to the type with its modifier (`bit(8)`), or to `varbit`.

### Q-731. PostgreSQL: an int where the server cannot infer a type is sent as text

**Kind:** silent. **Since daa8c35** (its symptoms), new here (the wrong answers). **Where:** [src/pg.rs](../../src/pg.rs) (about 1268, `_ => pg_text(v)` for TEXT and UNKNOWN parameters); statements are prepared without types (about 620).

| SQL, values | PostgreSQL | SQLite, MySQL |
|---|---|---|
| `select max(x) from (select ? as x union all select ?) s`, 10, 9 | `"9"` | `10` |
| `select greatest(?, ?)`, 10, 9 | `"9"` | `10` |
| `select substring('hello', ?, ?)`, 2, 3 | `null` | `"ell"` |
| `select ? as v`, 5 | `"5"` | `5` |
| `select ? is null`, `""` | could not determine data type of parameter $1 | (not compared) |
| `where int_col = ?`, 5000000000 | "cannot send int ... as int4" | no rows |

**Suggestion:** send an int as `int8` (and a float as `float8`) where the parameter's type is unknown, and let PostgreSQL cast; an int too large for the column's type is no match, not an error.

### Q-732. PostgreSQL json and jsonb: numbers past i64 come back as floats, and one value serde cannot read fails the whole query

**Kind:** silent, loud. **Since daa8c35,** with a new trigger. **Where:** `json()` in [src/pg.rs](../../src/pg.rs) (about 1261 and 1374), plain `serde_json` with its default limits.

```quillo
db.exec("create table r4 (id int, p jsonb)")
db.exec("insert into r4 values (1, '{\"n\": 18446744073709551615}'), (2, '{\"n\": 18446744073709551614}')")
let a = db.query("select p from r4 order by id")
print(a, a[0].p.n == a[1].p.n)     // both { n: 18446744073709552000 }, true
```

Writing the value back finds no row. With a row holding `1e400`, a `json` value with a lone `\ud800`, or a jsonb nested 128 levels or more (which PostgreSQL stores), `select` fails as a whole: "error deserializing column 0: number out of range" or "recursion limit exceeded". Every row is lost, not one.

**Suggestion:** read numbers past i64 as `dec`, and a value that cannot be read as an error value in that row's column (or as its text), not a failed query.

### Q-733. PostgreSQL: JSON text written to a json or jsonb column is stored as one JSON string, even with `?::jsonb`

**Kind:** silent. **Where:** [src/pg.rs](../../src/pg.rs) (about 1263, `v.to_json()` of a str).

```quillo
db.exec("insert into items (doc) values (?::jsonb)", json.stringify({ sku: "a1" }))
print(db.one("select jsonb_typeof(doc) as t, doc->>'sku' as sku from items"))   // { t: "string", sku: null }
```

MySQL's JSON column stores an object. Passing the map itself works; passing its text, which programs often have from a request, does not.

**Suggestion:** a str sent to a json or jsonb parameter is sent as the JSON text it is.

### Q-734. PostgreSQL: a float out of range for `real` or `float8` is stored as Infinity or 0

**Kind:** silent. **Since daa8c35** (`real`), new here (text). **Where:** [src/pg.rs](../../src/pg.rs) (about 1145, `put_f32(f as f32)`; about 1254, `parse::<f64>`).

`insert into r2 values (?)` into a `real` column with `1e39` stores `Infinity`, and with `1e-46` stores `0`. Text `"1e400"` for a `float8` stores `Infinity`, and `"1e-400"` stores `0`. PostgreSQL itself refuses both ("is out of range for type real/double precision", 22003), as MariaDB's `float` does.

**Suggestion:** an error when the value does not fit, as for ints.

### Q-735. SQLite: NaN is stored as NULL, and -0.0 as 0.0

**Kind:** silent. **Since daa8c35.** **Where:** [src/db.rs](../../src/db.rs) (about line 723).

```quillo
db.exec("create table m (y real)")
db.exec("insert into m (y) values (?)", sqrt(-1))
db.exec("insert into m (y) values (?)", -0.0)
print(db.query("select y, typeof(y) as t from m"))   // [{ y: null, t: "null" }, { y: 0.0, t: "real" }]
```

In a `not null` column it is a constraint error that does not name the NaN. PostgreSQL keeps both; MariaDB refuses NaN. The guide does not say.

**Suggestion:** refuse NaN with a message that names it (or document it), and keep -0.0.

### Q-736. MySQL: `db.open` of a database that does not exist creates it

**Kind:** silent. **Where:** `open_backend` passes `make = true` to `my::Pool::open_as` ([src/db.rs](../../src/db.rs) about 574, [src/my.rs](../../src/my.rs) about 134), which is meant for tenants.

`db.open("mysql://.../f55_typo")` gives an empty database, `{ d: "f55_typo", tables: 0 }`, and the database now exists: a typo in a URL is a new, empty production database. PostgreSQL says `database "f55_typo" does not exist` (3D000).

**Suggestion:** create only for tenants' databases; elsewhere, the server's error.

**Decided (2026-11-05):** each database's driver handles its database's quirks itself, so that a limitation of one database never reaches the program as Quillo's. Here the MySQL driver gives the same answer as PostgreSQL's for a database that is not there (an error), and creates one only for a tenant, inside the driver. The same rule covers the other differences between drivers in this report: Q-735, Q-739, Q-740, Q-741, Q-747, Q-748, Q-751, Q-752 and Q-753.

### Q-737. PostgreSQL: `?schema=` in a URL drops `?options=`

**Kind:** silent. **Where:** [src/pg.rs](../../src/pg.rs) (about line 77): `config.options(&search_path(s))` replaces the options.

With `?options=-c statement_timeout=100`, `show statement_timeout` is `100ms`. With `&schema=f55s` added it is `0`, and `pg_sleep(0.5)` runs to its end. Both options are in the guide.

**Suggestion:** append the search path to the options given.

### Q-738. `db.transaction` ignores misspelled option keys

**Kind:** silent. **Where:** [src/txn.rs](../../src/txn.rs) (about 78 and 95) read two keys.

`db.transaction(f, { isolaton: "serializable", retrys: 5 })` runs at `read committed` with no retries, and `check` says ok. A wrong value is refused; a wrong key is not.

**Suggestion:** refuse unknown keys, naming `isolation` and `retries`.

### Q-739. MySQL: `changes` counts rows changed, not rows matched

**Kind:** silent. **Where:** [src/my.rs](../../src/my.rs) (about 228) and `opts_of`.

`update stock set qty = 5 where sku = 'a'`, with `qty` already 5, gives `changes: 0` on MySQL and 1 on SQLite and PostgreSQL. The common "update, and insert when nothing changed" then fails with "Duplicate entry 'a'". An upsert's update counts 2. `?client_found_rows=true` gives 1.

**Suggestion:** set `CLIENT_FOUND_ROWS` by default, so `changes` means the same on all three.

### Q-740. MySQL: a `last_id` at or past 2^63 is negative

**Kind:** silent. **Where:** [src/my.rs](../../src/my.rs) (about 229, `u64 as i64`).

A `bigint unsigned auto_increment` at 9223372036854775808 gives `last_id: -9223372036854775808`, and looking the row up by it finds nothing.

**Suggestion:** a `dec` (or an error) past i64.

### Q-741. MariaDB: `BIT(n)` columns come back as raw bytes passed off as text

**Kind:** silent. **Since daa8c35.** **Where:** [src/my.rs](../../src/my.rs) (about 301).

`bit(8)` holding `b'00000101'`, `b'11111111'` and `b'01000001'` reads as `"\u0005"`, `"�"` and `"A"`; `bit(64)` of -1 is eight replacement characters, and `bit(1)` is `"\u0001"`. PostgreSQL's `bit` reads as `"0101"`. The guide does not mention BIT.

**Suggestion:** an int, or the same `"0101"` text PostgreSQL gives.

### Q-742. PostgreSQL: a type Quillo has no reader for comes back as control characters or hex, depending on the value; the guide's list is stale

**Kind:** silent, docs. **Since daa8c35.** **Where:** [src/pg.rs](../../src/pg.rs) (about 1409, `(None, Ok(s)) => Value::str(s)`).

The code's comment says "hex for the rest", but bytes that happen to be valid UTF-8 are returned as text: `'int4'::regtype` is `"\u0000\u0000\u0000\u0017"`; `'00:00:00+00'::timetz` is 12 NULs while `'12:00:00+02'::timetz` is `"\\x0000000a0eebb000ffffe3e0"`; `int4range`, `xid8`, `tsvector`, `pg_lsn` and composites give control characters. One column flips between the two forms from row to row. The guide (about line 5824) lists `inet` and `money` as arriving garbled; both read properly now.

**Suggestion:** always hex (or ask for the type's text form), and update the guide's list.

### Q-743. PostgreSQL: BC dates are read as text that cannot be written back

**Kind:** loud. **Since daa8c35.** **Where:** [src/pg.rs](../../src/pg.rs) (about 1374, `{:04}` on a negative year).

`'0044-03-15 BC'` reads `"-043-03-15"`, which written back is "time zone displacement out of range"; `'0001-06-01 BC'` reads `"0000-06-01"`, which is out of range. A BC `timestamptz` reads `"-4713-12-31T17:04:35Z"`.

**Suggestion:** a year written with its sign and four digits (`-0043`), as ISO 8601's expanded form, and the same read back.

### Q-744. PostgreSQL `real` and MariaDB `FLOAT` read back widened

**Kind:** silent. **Since daa8c35.** **Where:** [src/pg.rs](../../src/pg.rs) (about 1200) and [src/my.rs](../../src/my.rs) (about 287).

0.1 stored reads `0.10000000149011612` (psql and Python's drivers show 0.1); a `real` holding 3.4e38 reads `3.3999999521443642e+38`.

**Suggestion:** turn an f32 into the shortest f64 that prints the same (through its shortest decimal text).

### Q-745. PostgreSQL: after an `alter table`, kept prepared statements fail on every request until the server restarts

**Kind:** loud. **Where:** [src/pg.rs](../../src/pg.rs) (about 607 and 862) never drop a statement on 0A000.

After `alter table cp add column b`, `select * from cp` gives "cached plan must not change result type" (0A000). In a server, the connections that had prepared it fail every request (4 of 8 just after the alter, still failing seconds later), until a restart. A migration run by another instance is enough. SQLite and MySQL prepare again.

**Suggestion:** on 0A000, drop that connection's statement, prepare it again and retry once.

### Q-746. `db.exec` refuses function, procedure and trigger bodies as SQL that "begins or ends a transaction"

**Kind:** loud, a regression from a9b56bb (FEEDBACK44's refusal of hand-written `begin` and `commit`). **Where:** `statements()` in [src/db.rs](../../src/db.rs) (about 1105) splits on every `;`, ignoring `$$` and `BEGIN ... END`, and `controls_tx` then sees `end` (called about 1624).

```quillo
db.exec("create function f55_add(a int, b int) returns int language plpgsql as $$ begin return a + b; end $$")
db.exec("create trigger tg_bi before insert on tg for each row begin set new.n = new.id * 2; end")   // MySQL
```

Both stop the program with "this SQL begins or ends a transaction"; so do `do $$ ... $$` and `begin atomic`, inside `db.transaction` too. Migrations take the same SQL.

**Suggestion:** split statements the way the `?` scanner reads them (dollar quotes, strings, comments), and treat a `begin ... end` block after `create function/procedure/trigger` as one statement.

### Q-747. A map of values whose keys the SQL does not use fails when the SQL has no parameters

**Kind:** loud. **Since daa8c35.** **Where:** [src/db.rs](../../src/db.rs) (about 788: `parameter_count() > 0 &&` falls through to the count of positional values) and `translate` in [src/pg.rs](../../src/pg.rs).

```quillo
let f = { author: null }
let sql = "select id from r6" + (if f.author != null { " where author = :author" } else { "" })
print(db.query(sql, f))     // err("db: the query has 0 parameters, got 1")
```

SQLite and PostgreSQL; MySQL gives `[]`. `{}` fails the same way. The guide says "keys the SQL does not use are ignored", and the code's own comment speaks of "queries put together from optional parts".

**Suggestion:** a map with no `:name` in the SQL is ignored, on all three.

### Q-748. MySQL: a map or record as the only `?` value fails

**Kind:** loud. **Since daa8c35.** **Where:** [src/my.rs](../../src/my.rs) (about 329) takes any single map as named values.

`db.exec("insert into r7 (meta) values (?)", { tags: ["a"] })` gives "Statement takes 1 parameters but 0 was supplied". SQLite and PostgreSQL store `{"tags":["a"]}`, and so does MySQL with two or more values.

**Suggestion:** a single map is named values only when the SQL has a `:name`.

### Q-749. PostgreSQL: the `?` scanner misreads `?||`, `@?`, `?1` and nested comments

**Kind:** loud. **Since daa8c35** (`?1`, comments), new here (operators). **Where:** `is_question_operator` in [src/pg.rs](../../src/pg.rs) (about 1094).

`'ab' like ?||'%'` gives "operator does not exist: ?|| unknown", and the common `'%'||?||'%'` too. `jsonb @? '$.a'` is a syntax error, its `?` read as `$1`. `?1::int` becomes `$11`. A `?` in `/* /* ? */ ? */` is mis-scanned: PostgreSQL nests block comments.

**Suggestion:** a `?` followed by `||` is a placeholder; one after `@` is the operator; nested comments counted.

### Q-750. Read replicas: a `select` that writes takes the replica out of use for 10 s

**Kind:** perf. **Where:** `only_reads` in [src/db.rs](../../src/db.rs) (about 255).

`select nextval('rs')` goes to the replica, which says "cannot execute nextval() in a read-only transaction"; Quillo logs "a read replica failed", answers from the primary, and sends the next reads to the primary too, for 10 s. Any `select my_writing_fn()` does the same.

**Suggestion:** a read-only error from a replica sends that statement to the primary without marking the replica down.

### Q-751. MySQL: a value over `max_allowed_packet` fails as "Broken pipe" or "Packets out of sync"

**Kind:** loud. **Since daa8c35** (the message changed). **Where:** [src/my.rs](../../src/my.rs) (about 250).

`db.exec("insert into r11 values (?)", repeat("a", 20000000))` with `max_allowed_packet` at 16 MB gives "Input/output error: Broken pipe (os error 32)" or "Packets out of sync", in turn. PostgreSQL and SQLite took 40 MB.

**Suggestion:** compare the statement's size with the server's `max_allowed_packet` and say so.

### Q-752. SQLite's `@name` and `$name` work only when the map's key carries the sigil; on MySQL `@x` is silently null

**Kind:** docs. **Since daa8c35.** **Where:** [src/db.rs](../../src/db.rs) (about 791).

`db.one("select @x as v", { x: 3 })` is "no value for @x"; `{ "@x": 3 }` works. The guide's "SQLite also reads `@name` and `$name`" reads as plain keys. On MySQL `@x` is a session variable and gives `null`.

**Suggestion:** look `@x` and `$x` up as `x`, and say in the guide that on MySQL they are variables.

### Q-753. `last_id` and `changes` mean different things per database, and `db.exec("")` gives a stale count

**Kind:** docs. **Since daa8c35.**

- After `insert ... values (..), (..), (..)`, `last_id` is the last row on SQLite and the first on MySQL.
- `db.exec` of a `select`: SQLite gives the previous statement's `changes`, PostgreSQL the number of rows, MySQL 0; with values, SQLite refuses ("Execute returned results").
- `db.exec("")` and DDL on SQLite give the previous statement's `changes: 1`.
- `db.one("   ")` gives `err("db: not an error")` on SQLite and `null` on PostgreSQL.

**Suggestion:** document `last_id` per database; make `changes` 0 for statements that change nothing, and refuse empty SQL.

### Q-754. The guide's column-list `update` fails on MySQL, and on PostgreSQL with one column

**Kind:** docs. **Where:** USERGUIDE.md (about line 5379): `` update t set (${sql_name(cols)}) = (${sql_param(cols)}) ``.

MySQL gives a 1064 syntax error for any column list; PostgreSQL with one column gives "must be a sub-SELECT or ROW() expression" (0A000). SQLite works.

**Suggestion:** the guide's example as `set a = ?, b = ?`, built from the columns (or a helper that writes that form).

### Q-755. The Q-629 warning is wrong for PostgreSQL arrays

**Kind:** loud. **Where:** [src/check.rs](../../src/check.rs) (about 5311).

`where x = any(?)` and `?::jsonb[]` with a literal list warn "this list is one parameter, stored as its JSON text". On PostgreSQL it is an array, as the guide documents.

**Suggestion:** no warning when the `?` is in `any(...)`, `all(...)` or cast to an array type.

### Q-756. Smaller ones

**Kind:** silent.

- PostgreSQL arrays lose their lower bounds: `'[0:2]={1,2,3}'` reads `[1, 2, 3]`, so writing it back shifts every index ([src/pg.rs](../../src/pg.rs) about 1445).
- A `jsonb[]` parameter whose item is a list is taken for a sub-array ("malformed array literal").
- A MySQL URL with an IPv6 host, `mysql://root@[::1]:33655/...`, fails with "failed to lookup address information"; PostgreSQL takes it.
- Worse than the guide's note on MariaDB's `bigint unsigned` past 2^63: 2^64-1 and 2^64-2 read equal, and `where ubi = ?` with the value read matched 2 rows.
- `tinyint(1)` holding 2, 5 or -1 reads `true`.
- `sql_name("名前")` stops the program, though `sql_name`'s other refusals are error values; "letters" means ASCII.

## Components and forms

Each was driven in headless Chrome, key by key where typing matters, and run twice. `quillo check` says ok to every page here.

### Q-757. A two-way number input rewrites what is being typed

**Kind:** silent. **Where:** the `__live` microtask in `fire()`, then `liveApply` and `setProp`, which compares `el.value` with `display(v)` as text.

```quillo
component Price() {
    state amount = 0
    <div><input id="n" type="number" step="0.01" value={amount}/><p>{amount}</p></div>
}
```

Typed one key at a time: `1.05` becomes `15`, `0.001` becomes `1`, `-0.5` becomes `0.5`, `2.50` becomes `2.5`, `1e3` becomes `1000`. After `1.0` the state is 1, the input is set back to the text "1", and the next key is added to that.

**Suggestion:** do not write a value back to the input that is being typed in when it parses to the state's value (`Number(el.value) === v`), as React does for number inputs.

### Q-758. `<select value={x}>` shows the wrong option

**Kind:** silent. The server's HTML reproduced for this report. **Where:** the server writes `value` as an attribute of `<select>`; in the browser, `setProp` returns early while claiming, and `wire()` sets the select's own holes before its options have their values.

```quillo
component Pick() {
    state size = "m"
    <div><select id="s" value={size}><option value="s">S</option><option value="m">M</option></select><p>{size}</p></div>
}
```

- The server writes `<select id="s" value="m"><option value="s">S</option><option value="m">M</option></select>`. Browsers ignore `value` on a select, so the page shows S while the `<p>` says m, and it still shows S after the runtime starts.
- Made in the browser (behind `{show && ...}`), `selectedIndex` is -1: nothing is shown.
- Options from a `for` added after the value is set (state "c", then "c" added) still show "a"; `v = "r"` with new options in the same handler shows "q".

**Suggestion:** on the server, write `selected` on the matching option; in the browser, set the select's value after its children and again when its options change.

### Q-759. Rows keyed inside `? :` or `match` are not keyed

**Kind:** silent. **Where:** this compiles to `W.each(..., null, ...)`, kept by place, and each branch's `key` becomes `W.keyed`, so every row is made again.

```quillo
<ol>{for it in items => it.big ? <li key={it.id} class="big"><input/></li> : <li key={it.id}><input/></li>}</ol>
<button onclick={() => { items = drop(items, 1) + take(items, 1) }}>rot</button>
```

Type into the rows and press rot: every input is empty, and focus is lost. `match` does the same. React keys this form.

**Suggestion:** when every branch of the row's root has a `key`, key the list by it.

### Q-760. A text input or select bound to an int state makes it text

**Kind:** silent.

```quillo
component Q() {
    state qty = 1
    state n = 1
    <div><input value={qty}/><p>{qty + 1}</p>
    <select value={n}>{for i in [1, 2, 3] => <option value={i}>{i}</option>}</select><p>{n == 2 ? "two" : "not two"} {n + 1}</p></div>
}
```

Typing a 0 after the 1 shows `121`; picking option 2 shows `not two 21`. A `type="number"` input already gives a number back.

**Suggestion:** give back the state's own type (an int when the state is an int and the text parses), or have `check` warn for an int state bound to a text input or select.

### Q-761. `<select multiple value={list}>` never shows a selection, and each change keeps only the newest option

**Kind:** silent.

```quillo
<select multiple value={picked} onchange={(e) => { picked = js("Array.from", e.target.selectedOptions) | map((o) => o.value) }}>...three options...</select>
```

Nothing is ever shown selected. The server writes `value="[&quot;b&quot;]"`, and in the browser `fire()` sets `el.value` to the list's text, so each meta-click leaves only the newest option. `selected={contains(picked, ...)}` on each option works.

**Suggestion:** a list value on `<select multiple>` sets each option's `selected`, on both sides; or `check` refuses it and says to use `selected` per option.

### Q-762. A component that has left the page follows the parent's new props

**Kind:** silent. **Where:** `comp()` passes props as getters, and `dispose` does not freeze them.

```quillo
component Room(id: int, on_log) {
    <div onmount={() => { on_log("join " + str(id)); () => { on_log("leave " + str(id)) } }}>room {id}</div>
}
component App() {
    state id = 1
    state log = []
    <div><button id="next" onclick={() => { id += 1 }}>next</button>
    <Room key={id} id={id} on_log={(m) => { log = log + [m] }}/><p>{join(log, ", ")}</p></div>
}
```

After next: `join 1, leave 2, join 2`, where `leave 1` was meant. A `let` made from the prop has the new value too, though the old instance's `state` is right. A Save handler awaiting a server fn for record 1 when the key changes: the server saved 1, and the handler then says 2.

**Suggestion:** when a component is disposed, freeze its props at their last values.

### Q-763. A component whose root is an SVG element is invisible inside `<svg>`

**Kind:** silent.

```quillo
component Bar(h: int) { <rect class="bar" width="10" height={h} fill="teal"/> }
view Chart() = <svg width="100" height="100"><Bar h={40}/><Bar h={70}/></svg>
```

Both bars are 0x0 inside a `quillo-island` element, which SVG does not draw. The same when the component is made in the browser (`{on && <Dot/>}`).

**Suggestion:** `data-qi` on the SVG root element itself, as Q-011 did for `<tr>`.

### Q-764. A bare assignment statement in a component runs once, not as an effect

**Kind:** silent.

```quillo
component T() {
    state items = []
    state count = 0
    state count2 = 0
    count = len(items)
    if true { count2 = len(items) }
    <div><p>{count} {count2}</p><button onclick={() => { items = items + ["x"] }}>add</button></div>
}
```

Shows `0 1`, then `0 2`: `count = len(items)` compiles to one `$count.set(...)`, while the `if` becomes an effect. The guide says a statement that reads something is an effect.

**Suggestion:** a top-level assignment that reads state is an effect too (or `check` says to write it as `let`).

### Q-765. A range input with a dynamic `max` has its `value` clamped to the default maximum

**Kind:** silent. The same order as Q-758: the value is set before `max`.

```quillo
state v = 500
state hi = 1000
<input type="range" value={v} min="0" max={hi}/>
<button onclick={() => { hi = 2000; v = 1500 }}>raise</button>
```

Made in the browser it shows 100 while `v` is 500; after raise, both the server's and the browser's inputs show 1000 while `v` is 1500. With `max` written before `value` it works.

**Suggestion:** set `value` after `min`, `max` and `step`, and again when they change.

### Q-766. Without `Element.moveBefore` (Safari), a focused keyed row that moves loses focus

**Kind:** silent. **Where:** the fallback in `moveNode`.

Rows of `<input onkeydown={...}>` where ArrowUp moves its row up; `moveBefore` removed before load, as in Safari. After one ArrowUp the focus is on the body, and a second does nothing. The guide says a keyed row's "DOM, focus and what was typed go with them"; Q-343 covered pointer capture only.

**Suggestion:** save the focused element and its selection around a move, and restore them, as React does.

### Q-767. `e.form` leaves out the submit button that was pressed

**Kind:** silent. **Where:** `eventField`'s `"form"` case.

```quillo
<form onsubmit={(e) => { got = json.stringify(e.form) }}><input name="title" value="t1"/>
<button name="action" value="save">Save</button><button name="action" value="delete">Delete</button></form>
```

Delete gives `{"title":"t1"}`, with no `action`. `e.submitter.value` works.

**Suggestion:** `new FormData(form, e.submitter)`.

### Q-768. A form reset empties inputs that were made in the browser

**Kind:** silent. **Where:** `blockTemplate`'s `make` sets the `value` property, not the attribute.

`<form><input value="default"/> ... <button type="reset">` shown through `{show && ...}`: reset empties the input, while the same form from the server resets it to "default". `getAttribute("value")` is null in the browser's copy.

**Suggestion:** set the attribute too when an input is first made (its default value).

### Q-769. `<template>` in a component loses its content, with a mismatch on every load

**Kind:** silent. **Where:** `claimEl` and `blockTemplate` walk the element's children, not its `.content`.

`<div><input/><template id="tpl"><li class="t">row</li></template></div>` reports a mismatch on every load ("want li, have null"); afterwards `tpl.content` holds no nodes and `tpl.childNodes` one.

**Suggestion:** treat a `<template>`'s children as its `.content`, or leave its content as the server wrote it.

### Q-770. A child that sets the parent's state as it starts makes the server's page show two values

**Kind:** silent, small.

`<p>{count}</p><Child on_ready={(n) => { count = n }}/><p>{count}</p>`, the child calling `on_ready(5)`: the server writes 0 above the child and 5 below it, with `data-s` holding 0. The browser then shows 5 and 5.

**Suggestion:** on the server, render the component again when a child changed its state, or warn that a child's start writes the parent's state.

### Q-771. `autofocus` on markup that appears later does nothing, and the guide does not say so

**Kind:** docs. `{show && <input autofocus/>}` after a click leaves the focus on the button. That is what browsers do with inserted elements, but React and Svelte call `.focus()` on mount, and someone coming from them expects it.

**Suggestion:** focus on mount (as they do), or say so in the guide with the `onmount={(el) => el.focus()}` form.

## Server and browser

1,623 distinct expressions, 46 of them also crossed as a prop and as a state: 1,945 cases, rendered by the server in a component and computed again by the browser, then compared; a page with one island per case and a `MutationObserver` recorded which islands the browser made again. Run twice, with the time zones changed between the runs (the server in Los Angeles, Kolkata and Paris, Chrome in Kolkata, Chatham and São Paulo). 1,673 were the same. Of the 272 that differed, 53 are known or documented (a whole float's `.0` in `json.stringify`, the last digits of `math.*`, the wording of `json.parse`'s errors, NaN and inf as null in props, Q-302's key order) and 3 are time zone names one side does not know. The other 216 are below, by cause.

### Q-772. A CR or NUL in text makes the browser re-make the component on every load

**Kind:** silent. The server's text reproduced for this report. **Where:** text is written with `\r` and `\0` as they are; Q-312's `&#13;` covers attribute values only ([src/interp.rs](../../src/interp.rs) about 4815).

The HTML parser turns CR into LF and drops NUL, so the server's page no longer says what the browser computes.

| Text | Server's page, as parsed | Browser |
|---|---|---|
| `"line one\r\nline two"` | `line one\nline two` | `line one\r\nline two` |
| `"a\rb"` | `a\nb` | `a\rb` |
| `"a\u0000b"` | `ab` (U+FFFD in an attribute) | `a\x00b` |

The real case: two lines typed in a `<textarea>` and posted by a form arrive with CRLF. A component that shows the note is made again on every load. The same as a prop and as a state.

**Suggestion:** write `&#13;` for CR in text too, and U+FFFD (or `&#xFFFD;`) for NUL on both sides.

### Q-773. Errors the server hides in production are shown in full once the browser re-makes the component

**Kind:** silent, security. The server's text reproduced for this report. **Where:** `repr_at` with `hiding()` in [src/value.rs](../../src/value.rs), against `repr` in runtime.js.

| Expression | Server (no `--watch`) | Browser |
|---|---|---|
| `str(int(v("abc")))` | `err("internal error")` | `err("not an int: \"abc\"")` |
| `` `could not read: ${int(x)}` `` | `could not read: err("internal error")` | `could not read: err("not an int: ...")` |
| `str(json.parse("[1,]"))` | `err("internal error")` | `err("invalid JSON: Unexpected token ...")` |

The two differ, so the browser makes the component again, and the visitor reads the text Q-197 meant to hide. Under `--watch` both sides show it, so the developer never sees the difference. On the server alone, `error(r)` and `r.message` are not hidden at all.

**Suggestion:** the runtime hides a builtin's error text in production as the server does (the "own" mark from Q-197 already crosses the wire for services).

### Q-774. Attributes are not compared when a component starts in the browser

**Kind:** silent. Every difference in this section was a mismatch when shown as text. The same expression in an attribute (`title={...}`) was not reported: the browser's value silently took the server's place.

**Suggestion:** compare attribute holes as text holes are compared, and report a difference the same way, under `--watch` at least.

### Q-775. A whole float is an int in the browser, and code does other things

**Kind:** silent. Q-013 fixed the text only. **Where:** `typeIs` in runtime.js (about line 452: `"float"` is any number), and `div` and `arith`, which take a whole float with a zero divisor for an int divided by zero.

| Expression | Server | Browser |
|---|---|---|
| `v(5.0) / v(0)`, `v(1) / v(0.0)`, `v(0.0) / v(0.0)` | `inf`, `inf`, `NaN` | stops with "division by zero"; the place is emptied |
| `match v(2) { float as f => "float", int as i => "int" }` | `int` | `float` |
| `type_of(2.0)`, `type_of(sqrt(4))`, `type_of(json.parse("1e2"))` | `float` | `int` |
| `fn f(x: int)` given `2.0`; `fn g(x: float)` given `2` | stops | accepted |
| `Pt({ n: 2.0 })` with `n: int` | `err("Pt: n must be int ...")` | `{ n: 2 }` |
| `xs[v(1.0)]`, `slice(s, 1.0, 3.0)`, `range(3.0)`, `repeat`, `take`, `round(x, 2.0)` | stops | works |
| `v(9007199254740993) * v(1.0)` | `9007199254740992` | `9007199254740993` |

**Suggestion:** a float that crosses to the browser keeps its type (a wrapper, as decimals have, or a mark in the state's JSON), and `typeIs`, `div` and `arith` read it.

**Decided (2026-11-05):** numbers must be accurate and secure, money above all. The browser gives the server's answer for every number; where it cannot, it refuses with an error, and never shows a different number. So the type is kept (not a note in the guide), and amounts never go through a float on either side. Q-776, Q-777, Q-781, Q-827 and the server's own Q-787 to Q-790 follow the same rule.

### Q-776. Ints past 2^53 in the browser: math functions stop, `factorial(19)` overflows, `==` with an equal float is false

**Kind:** silent. **Where:** BigInts reach functions that take numbers; `overflowed()` (runtime.js about 2717); `eq`, which never compares a BigInt with a number.

| Expression | Server | Browser |
|---|---|---|
| `sqrt`, `ceil`, `floor`, `round`, `math.sqrt`, `log`, `isqrt`, `gcd` of `9007199254740993` | a value | stops |
| `stats.mean`, `median`, `vec.add`, `mul`, `cumsum`, `range(big, big + 2)` | a value | stops |
| `math.factorial(19)`, `math.comb(60, 30)`, `math.prod([2^32, 2^22])` | exact | "integer overflow" |
| `v(9007199254740992) == v(9007199254740992.0)`, also in `contains`, `index_of`, `remove`, `unique`, a `match` literal | `true` | `false` |
| `sort([9007199254740993, 1.5, 9007199254740992.0])` | `[1.5, ...993, ...992]` | `[1.5, ...992, ...993]` |
| `v(3037000500) * v(3037000500)` | integer overflow | `9223372037000250000` |
| `int(-9.223372036854775808e18)`; `json.parse("-9223372036854775808")` | `-9223372036854775808` | `err(... too large)`; `-9223372036854776000` |

Q-787 is the server's own part of this.

**Suggestion:** BigInt paths in the math and `vec` functions, `eq` comparing a BigInt with a number exactly, and `big()` checking the i64 range on both ends.

**Decided (2026-11-05):** as for Q-775: exact on both sides, or an error, never another number.

### Q-777. The shortest digits of a float break ties the other way

**Kind:** silent. **Where:** `js_number` in [src/value.rs](../../src/value.rs) (about 734) takes Rust's shortest digits; JavaScript takes the even digit on a tie.

`1916218016406818.3` is `...818.3` on the server and `...818.2` in the browser; `1e15 + 0.3` is `...000.3` and `...000.2`. Of 200,000 random non-whole floats, 1,137 differ, all at or above 2^37; in [2^50, 2^51), where microsecond timestamps fall, 34% differ.

**Suggestion:** on a tie, take the digit JavaScript takes (ECMAScript's Number::toString rule), so the server writes what the browser writes.

### Q-778. String functions differ in the browser: order by UTF-16 unit, `trim`'s set, `replace(s, "", x)`

**Kind:** silent. **Where:** runtime.js `cmp` (about 655, `a < b`), `trim` (about 1385, JavaScript's `trim`), `replace` (about 1399, `split(a).join(b)`).

| Expression | Server | Browser |
|---|---|---|
| `join(sort(["Ａ", "😀"]), " ")` | `Ａ 😀` | `😀 Ａ` |
| `"😀" < "\uffff"`; `max(["😀", "\uffff"])` | `false`; `😀` | `true`; `\uffff` |
| `len(trim("\ufeffname"))` | `5` | `4` |
| `len(trim("name\u0085"))` | `4` | `5` |
| `replace("abc", "", "-")` | `-a-b-c-` | `a-b-c` |
| `replace("😀", "", "-")` | `-😀-` | `\ud83d-\ude00` (lone surrogates) |

The guide says strings order by code point. Characters from U+E000 to U+FFFF (full-width forms, `ﬁ`, half-width katakana) against emoji and other astral characters sort differently. `replace` with an empty pattern on an empty string gives `""` in the browser.

**Suggestion:** compare by code point in `cmp`, trim the server's set, and `replace` by code points with an empty pattern, as the server does.

### Q-779. HTML values made text in the browser are `"<html>"`, and `json.stringify` writes them empty

**Kind:** silent. **Where:** `repr` and `toJSON` in runtime.js.

| Expression | Server | Browser |
|---|---|---|
| `str(<b>x</b>)`, `"" + h`, `${h}` | `<b>x</b>` | `<html>` |
| `json.stringify({ h: <b>x</b> })` | `{"h":"<b>x</b>"}` | `{"h":""}` |
| `len(h)`; `h == h` | `8`; `true` | stops; `false` |

**Suggestion:** an HTML value's text is its markup on both sides.

### Q-780. NaN is refused for ordering on the server and ordered in the browser

**Kind:** silent. `sort([NaN, 1])`, `max`, `min`, `sort_by` by NaN, `NaN < 1` and `stats.median` stop on the server (FEEDBACK49's Q-605 fix); the browser gives `[NaN, 1]`, `NaN`, `1` and `false`. `vec.argsort([NaN, 1])` is `[1, 0]` on the server and `[0, 1]` in the browser.

**Suggestion:** the same refusal in runtime.js.

### Q-781. `json.stringify` turns to exponent form at other sizes on each side

**Kind:** silent. The server writes an exponent from 1e16 up and below 1e-5; JavaScript from 1e21 up and below 1e-6. `1e16` is `1e+16` and `10000000000000000`; `0.0000015` is `1.5e-6` and `0.0000015`; `1e20` is `1e+20` and `100000000000000000000`. Q-309 covered `str` and the `.0`, not this.

**Suggestion:** JavaScript's thresholds on the server, as for `str`.

### Q-782. Smaller differences

**Kind:** silent.

- `json.parse("1e400")` is an error on the server and `inf` in the browser; `"\"\\ud83d\""` is an error on the server and a lone surrogate in the browser.
- `??` misses a stop in a field or an index to its left in the browser: `v("abc").foo ?? "none"` and `[1, 2][v(1.5)] ?? 10` give the fallback on the server; the browser empties the place and logs an error ([src/client.rs](../../src/client.rs) about 3122: `Coalesce` leaves `Field` and `Index` unguarded).
- `str()` of a map with non-ASCII letters in its keys: `{ é: 1 }` on the server, `{ "é": 1 }` in the browser (also Ω, 日本, a², ǅ; `fmtKey` takes ASCII only).
- `float("0x10")`, `"0b11"`, `"0o17"` and `" 0x10 "` are errors on the server and `16`, `3`, `15` and `16` in the browser (`coerce` uses `Number()`).
- With the `m` flag, U+2028 and U+2029 end a line for `^` and `$` in the browser only.
- `posixrules` is a zone on the server only; `US/Pacific-New` and `Canada/East-Saskatchewan` in the browser only. (Offsets as zones are Q-829.)

## Core values

`quillo check` passes every repro in this section. Where the guide says Quillo follows JavaScript or Python, the table gives what they do.

### Q-783. `sum` of text joins it after a "0"

**Kind:** silent. Reproduced for this report. **Where:** `sum` in [src/builtins.rs](../../src/builtins.rs) (about 651) falls back to `binop(Add)`; runtime.js (about 1427) does the same.

```quillo
let amounts = map(split("apples,12.50\npears,5.00", "\n"), (l) => split(l, ",")[1])
print(sum(amounts))        // 012.505.00
print(sum([1, 2, "3"]))    // 33
```

Python raises `TypeError`. Values read from CSV, forms or the session are text, so this is the common case. Q-374 saw "sum joins it from 0" in passing; its fix went to the session.

**Suggestion:** `sum` refuses text ("`sum` adds numbers; got the str "12.50" at item 1: use `float(...)` or `dec(...)` first").

### Q-784. Error values silently become text

**Kind:** silent. **Where:** `key_text` in [src/builtins.rs](../../src/builtins.rs) (about 2283), the `re.replace` callback path, `url_encode`.

```quillo
print(group_by(["1", "x"], (s) => int(s)))       // a key "err(\"not an int: \\\"x\\\"\")"
print(set({}, int("q"), 1))                      // the error's text as a key
print(re.replace("ab", "a", (m) => err("bad")))  // err("bad")b
print(url_encode({ a: err("e") }))               // a=err%28%22e%22%29
```

The guide's rule is that a builtin given an error where it wants something else says what the error was, and `filter`, `find`, `any`, `all` and `count` give back their function's error. A `?` inside a `re.replace` callback writes "err(...)" into the text.

**Suggestion:** each of these gives the error back.

### Q-785. A pattern that binds one name twice matches anything

**Kind:** silent. Reproduced for this report.

```quillo
fn same(v) = match v {
    [x, x] => "same"
    _ => "different"
}
print(same([1, 2]))     // same
let [y, y] = [1, 2]     // y is 2
```

`{ p: a, q: a }` matches `{ p: 1, q: 2 }` with `a` 2. Python refuses ("multiple assignments to name 'x' in pattern"), Rust too (E0416); Erlang and Elixir test the two for equality. Q-508 covered parameters and fields, not patterns.

**Suggestion:** an error in `check` and the parser: "`x` is bound twice in this pattern; to test that two parts are equal, use `[x, y] if x == y`".

### Q-786. Builtins ignore extra arguments when the program runs

**Kind:** silent. **Where:** `Interp::call` in [src/interp.rs](../../src/interp.rs) (about 3500) gives a builtin its arguments without counting them.

```quillo
fn desc(xs) = xs.sort((a, b) => b - a)
fn two(line) = line.split(",", 2)
print(desc([3, 1, 2]), two("a,b,c"))   // [1, 2, 3] ["a", "b", "c"]
```

`check` finds it only when it knows the receiver's type ("`sort` takes 1 argument, got 2" on a literal). On an untyped parameter, a variable holding a builtin (`let f = split`) or `fs[0](...)`, the extra arguments are dropped. JavaScript gives `[3, 2, 1]` and `["a", "b"]`; Python raises `TypeError`.

**Suggestion:** count a builtin's arguments when it is called, and say what it takes (for `sort` with a function: "use `sort_by`").

### Q-787. An int and a float are compared through f64

**Kind:** silent. **Where:** [src/value.rs](../../src/value.rs) (about 824, `a.as_f64() == b.as_f64()`, and 870).

```quillo
print(9007199254740993 == 9007199254740992.0)                          // true
print(sort([9007199254740993, 9007199254740992.0, 9007199254740992]))  // [...993, ...992.0, ...992]
print(unique([9007199254740993, 9007199254740992.0]))                  // one item
```

Python compares exactly (`False`; `sorted` gives `[...992.0, ...992, ...993]`). In random lists of 2,000 mixed values near 2^53, half of 30 runs left 60 to 150 ints out of order. Q-776 is the browser's side.

**Suggestion:** compare an int with a float exactly, as Python does.

### Q-788. `floor(a / b)` gives wrong digits past 2^53, though the guide calls it integer division

**Kind:** silent. **Where:** [src/interp.rs](../../src/interp.rs) (about 1352, `x as f64 / y as f64`); `to_int_or_float` ([src/builtins.rs](../../src/builtins.rs) about 2106) keeps a float above 9e15.

```quillo
let a = 9007199254740995
print(floor(a / 2), type_of(floor(a / 2)))   // 4503599627370498 int (Python a // 2: 4503599627370497)
print(floor(9223372036854775807 / 10))       // 922337203685477600, a float
```

Nanosecond timestamps (about 1.7e18) and amounts in micro-units are past 2^53. `(a - a % b) / b` is exact.

**Suggestion:** `floor`, `ceil` and `round` of an int division done in integers; or an integer-division operator or `div` builtin, and the guide pointing to it.

### Q-789. `round(x, n)` ignores a negative `n` for floats, and rounds big ints through a float

**Kind:** silent. **Where:** [src/builtins.rs](../../src/builtins.rs) (about 2145, `.clamp(0, 15)`); runtime.js (about 1752) the same.

```quillo
print(round(1234.5678, -2), round(1255, -1))   // 1235.0 1255.0 (Python: 1200.0, 1260)
print(round(1234.5m, -2))                      // 1200: decimals do it
print(round(9007199254740993, 2))              // 9007199254740992.0 (Python: 9007199254740993)
```

`type_of(round(1e16))` is float, though the guide says `round(x)` gives an int.

**Suggestion:** negative digits for floats and ints as decimals have them; an int rounded to whole digits stays exact.

### Q-790. `pow(int, int)` with an exponent of 2^32 or more falls back to a float

**Kind:** silent. **Where:** [src/builtins.rs](../../src/builtins.rs) (about 2200): `u32::try_from(y)` fails, then `powf`.

`pow(-1, 9007199254740993)` is `1.0` (wrong sign); `pow(2, 4294967296)` is `inf`, where the guide says an overflow, as `*` gives; `pow(0, 4294967296)` and `pow(1, 4294967296)` are the floats `0.0` and `1.0`.

**Suggestion:** for an int base of -1, 0 or 1 the exact int; otherwise the overflow error.

### Q-791. `unique` is quadratic

**Kind:** perf. Reproduced for this report. **Where:** [src/builtins.rs](../../src/builtins.rs) (about 2210, `out.iter().any(equal)`); runtime.js (about 1794) the same.

`unique(range(0, 20000))` takes 1.38 s; 40,000 ints take 5.6 to 6.0 s and 40,000 strings 7.2 s. Python's `list(dict.fromkeys(...))` takes 0.4 ms for 20,000. Quillo must be faster than Python.

**Suggestion:** a hash of each item's key (as `group_by` keys it), with `==` checked on a collision.

### Q-792. `json.stringify` writes functions as strings

**Kind:** silent. `json.stringify({ f: (x) => x, g: print })` gives `{"f":"<fn <fn>>","g":"<builtin print>"}`, and a route's answer does the same. JavaScript leaves the keys out; Python raises `TypeError`. The guide lists how decimals, NaN and errors are written, not functions.

**Suggestion:** an error, naming the key.

### Q-793. `math.trunc`, `floor` and `ceil` refuse values between 9.2e18 and 2^63

**Kind:** loud. **Where:** [src/numeric.rs](../../src/numeric.rs) (about 1068: `y.abs() >= 9.2e18`, where 2^63 is about 9.223e18). `math.trunc(9.2e18)` is "integer overflow", while `int(9.2e18)` is `9200000000000000000`.

**Suggestion:** compare with 2^63 (`9223372036854775808.0`).

### Q-794. A map literal with a key written twice gets no warning

**Kind:** loud. `{ a: 1, b: 2, a: 3 }` becomes `{ a: 3, b: 2 }`, as do `{ a: 1, "a": 2 }` and `{ a, a: 2 }`, and `check` says ok. TypeScript and ESLint (`no-dupe-keys`) flag it.

**Suggestion:** a warning in `check` (a spread before the key is not a duplicate).

### Q-795. The smallest int cannot be written as a literal, and the error gives no hint

**Kind:** loud. `-9223372036854775808` is "number `9223372036854775808` is too large". Rust, Java, JavaScript and Python take it.

**Suggestion:** take `-` followed by 9223372036854775808 as the smallest int, or say "write `-9223372036854775807 - 1`".

Not counted: the "written for a newer Quillo" note still follows `\x41`, `\u{}`, `\u12` and `\U0001F600` (Q-008 seen again). `"😀"`, the JavaScript and JSON way to write an emoji, is refused as "`\u{D83D}` is not a Unicode character", with no word about `\u{1F600}`. Unknown-name hints go by spelling, not meaning (`avg` suggests `abs`, `chunk` suggests `count`; `entries` and `includes` get none). `slice` of a 100,000-character string costs about 58 µs a call.

## Concurrency, live data, jobs

Each race was run three times or more, and the counts are given.

### Q-796. `set_cookie`, `set_session` and `end_session` inside `parallel` are dropped

**Kind:** silent, security. Reproduced for this report. **Where:** `run_parallel` in [src/interp.rs](../../src/interp.rs) (its in-task branch): `task_fork()` and `fork()` start each child with `set_cookies: Vec::new()` and `session: None`, and nothing copies them back.

```quillo
route GET /c = { parallel(() => set_cookie("a", "1"), () => set_cookie("b", "2")); "ok" }
route GET /s = { parallel(() => set_session({ a: 1 }), () => 2); "ok" }
route GET /e = { parallel(() => end_session(), () => 2); "ok" }
```

None of the three answers has a `Set-Cookie` (3 of 3 runs each); outside `parallel` the cookie is sent. `/e` is a logout that leaves the visitor signed in. The guide does not say a child cannot set cookies.

**Suggestion:** merge the children's cookies and session changes back into the request, in the children's order; or refuse these calls inside `parallel` with an error.

### Q-797. A global changed in a transaction that rolls back keeps its value, but live pages are never told

**Kind:** silent. **Where:** `publish()` in [src/interp.rs](../../src/interp.rs): inside a transaction it queues to `self.pending`, which the rollback cuts.

```quillo
db.open(":memory:")
db.exec("create table t (id integer primary key, v int)")
let count = 0
server fn load_count() -> int = count
route GET /tx = { db.transaction(() => { count += 1; db.exec("insert into t (v) values (1)"); err("no") }); count }
```

With 50 live pages open, `GET /tx` leaves the server's `count` at 1, while all 50 pages get no frame and keep showing 0 (3 servers, 50 of 50 each time). The same when the transaction fails by a run-time error, and when only an inner savepoint rolls back. The next ordinary write puts them right. The guide says globals are not rolled back, that assignments to globals are announced, and that the announcement is dropped on rollback; the value survives the rollback and its announcement does not.

**Suggestion:** announce `var:` topics at once (or at the transaction's end, whatever its outcome), since the value is kept either way.

### Q-798. A cron job's first due time after a start is skipped

**Kind:** silent. **Where:** `start()` in [src/jobs.rs](../../src/jobs.rs): `last.entry(name).or_insert(due)` is first reached in the first minute that matches, which then counts as "first seen".

With the server started at 04:01:06 UTC: `schedule("2 * * * *", a)` and `schedule("2 4 * * *", b)` did not run at 04:02; `schedule("2-4 * * * *", e)` ran at 04:03 and 04:04 only. On a second server `"0 * * * *"` skipped 04:00, and `"0,1 * * * *"` ran at 04:01 only. `"1m"` and `"* * * * *"` were on time. So `"0 3 * * *"` and `@daily` first run a whole period late, and a server deployed every day before 03:00 never runs its nightly job. The guide: "waits for its next time; it never runs at once".

**Suggestion:** record the start time as the last time seen when the job is registered, so the first matching minute after it runs.

### Q-799. Channel `join` and `leave` do not pair

**Kind:** silent. **Where:** `from_page` in [src/wt.rs](../../src/wt.rs): the `"j"` arm checks `p.rooms`, not `joining()`; the `"l"` arm ignores a join still waiting; `joined()` does nothing when the peer has gone.

```quillo
let online = 0
channel.on("room", (m, p) => {}, {
    join: (peer) => { sleep(150); online += 1; true },
    leave: (peer) => { online -= 1 },
})
route GET /stats = { online, peers: channel.peers("room").len }
```

A page that sends `["j","room"]` and closes within 40 ms: 10 pages a run gave 10 joins and no leaves each run, so `online` reached 10, 20 and 30 with no peers. `["j"]` twice runs `join` twice and `leave` once. `["j"],["l"],["j"]`, which runtime.js sends when a component goes and comes back before its join is answered (about lines 3705 and 3717), does the same. `["j"],["l"]` leaves the page in the room until it closes. The guide's lobby example broadcasts "joined" in `join` and "left" in `leave`.

**Suggestion:** one join per page and room at a time; a leave or a close while a join is waiting runs `leave` once the join has run (or cancels it).

### Q-800. Server fn answers are applied in the order they finish

**Kind:** silent, docs. **Where:** a server fn's answer in runtime.js has no "latest wins" per state.

```quillo
server fn search(q: str) -> str { sleep(max([0, 400 - q.len * 80])); `results for ${q}` }
component Search() {
    state q = ""
    state shown = ""
    <div><input value={q} oninput={(e) => { q = e.target.value; shown = search(q) }}/><p>{shown}</p></div>
}
```

Typing "abcd" in Chrome ends on "results for a" (3 of 3). Five quick clicks calling `echo(k, (6 - k) * 80)` end on `last = 1`. With `state shown = live(search, q)` it ends on "results for abcd". The guide's "calls run one after another" holds within one handler; overlapping handlers are not mentioned, and `live` is not given as the way.

**Suggestion:** a state set from a server fn call takes only the latest call's answer (dropping older ones), or the guide says so and points to `live(...)`.

**Decided (2026-11-05):** the latest call only. An older call's answer that arrives after a newer call was made is dropped for that state.

### Q-801. Topic names are not checked

**Kind:** silent.

- **A comma:** `watch("search:a,b")` and then `publish("search:a,b")` sends no frame: `live_query` in [src/server.rs](../../src/server.rs) splits `t=` on commas, and `liveLoop` joins the topics with commas.
- **A trailing `:*`** is a wildcard ([src/shared.rs](../../src/shared.rs) `is_wildcard`): `publish("room:*")` wakes `room:7` and drops the shared runs under it, and `publish("db:*")` wakes every table's pages. It is not in the guide, so a topic built from a visitor's text that ends in `:*` wakes everything under it.
- **Length:** a 70,000-character topic, or 100 topics of 120 "é" (a 72,787-byte URL), gets 414 on the live socket, and the page never updates ([src/httpd.rs](../../src/httpd.rs) `MAX_HEAD`, 64 KiB).
- **Empty:** `watch("")` never wakes.

**Suggestion:** escape topics on the wire; document `:*` or make it a separate call; refuse an empty topic and one longer than a limit, with an error at `watch`.

**Decided (2026-11-05):** the `:*` wildcard is meant for programs (other apps use it), so it stays and goes in the guide, with a warning that a topic built from a visitor's text must not end in `:*`. The comma, the length and the empty topic are still to fix.

### Q-802. `sleep` holds a wait thread for its whole length

**Kind:** perf. **Where:** `sleep` in [src/builtins.rs](../../src/builtins.rs), through `unlocked` and `httpd::wait_elsewhere`.

With the guide's `/feed` (`events` with `sleep(1000)` in its loop) and `route GET /napping = { sleep(10); "ok" }`: with 65 listeners, `/napping` takes 506 to 904 ms; with 100, 512 to 901 ms; with 60 to 64, 11 to 51 ms; with `--workers 256` and 100 listeners, 13 to 34 ms. The guide: "the rest of the server carries on meanwhile ... holding up no other request".

**Suggestion:** a sleep that holds no thread (a timer that wakes the request), or a pool of waiting threads that grows.

### Q-803. `every` and `schedule` called in a route answer 200 and never run; `check` passes what the start refuses

**Kind:** silent. **Where:** `every_at_top` and `schedule` in [src/jobs.rs](../../src/jobs.rs), and [src/check.rs](../../src/check.rs).

`route GET /e = { every(50, tick); "added" }` and `route GET /s = { schedule("1s", job); "added" }` answer "added", and the tick and the job never run (2.5 and 3 s later, 2 runs). `check` also passes these, which are refused only when the program starts: `every(0)`, `every(-5)`, `every(0.5)`; cron fields `60`, `*/0`, `5-1`, `MON`, `JAN`; `@reboot`; 6-field lines; tabs; a job name twice; a bad `{ name }`.

**Suggestion:** an error for `every` and `schedule` outside the top level, and `check` reading their literal arguments as the start does.

### Q-804. One failing `parallel` function does not stop the others, though the guide says it does

**Kind:** docs. `parallel(() => slow(300, 1), () => boom(), () => slow(600, 2))` answers 500 after 610 ms, and both other functions' writes are kept (3 of 3). The guide: "a failure stops the whole call". `run_parallel` runs every child to its end, then raises the first error.

**Suggestion:** the guide says what happens (the others finish, and the first error is raised), or the others are stopped at their next step.

### Q-805. Two requests of one visitor writing the session at once: the last cookie wins, and the guide does not warn

**Kind:** docs. 20 requests at once with one cookie, each `set_session({ ...s, cart: (s.cart ?? []) + [i] })`: each answer's cookie holds one item, and 19 of the 20 adds are lost (3 of 3). This comes with sessions kept in a cookie.

**Suggestion:** say so where sessions are described, and point to a table for data that must not be lost.

### Q-806. Over a WebSocket a burst drops channel messages even for a page reading at full speed

**Kind:** docs. One request broadcasting 20,000 messages: a page reading at full speed got 13,378, then 14,028; a paused one 12,979 and 13,266. `quillo_channel_dropped_total` was 13,643, then 26,349. The guide says the WebSocket "carries every message reliably and in order", and elsewhere describes the 4,096-message queue that drops.

**Suggestion:** say "in order, and while the page keeps up", next to the queue's limit; or make `broadcast` wait (back pressure) for reliable messages.

## Files of a program, plugins, native Rust

Rust modules were built by `quillo run` into `~/.cache/quillo/native`, not the repository's `target/`.

### Q-807. A private function's tag depends on the working directory: copies started from different folders all run a `schedule` job

**Kind:** silent. Reproduced for this report. **Where:** `tag()` in [src/privacy.rs](../../src/privacy.rs) (line 25) makes the path relative to `std::env::current_dir()` (line 29); its comment (line 24) says the tag is "the same on every server and after every deploy". The same tag renames names two files share ([src/modules.rs](../../src/modules.rs) about 183).

```quillo
// lib/t.qwe
private fn tick() = 1
fn name_of() = str(tick)
// main.qwe
use lib/t.qwe
print(name_of())
```

From the program's folder this prints `<fn tick__pv9f60406a>`, from its parent `<fn tick__pvbd9f335b>`, from `/tmp` `<fn tick__pv9a7bcc37>`. With `schedule("1s", tick)` in `lib/jobs.qwe` and two copies on one database, copies started from the same folder take turns, and copies started from different folders both run the job every second.

**Suggestion:** make the path relative to the program's main folder (the folder of the file given to `quillo run`).

### Q-808. A browser name taken from two scripts: the last wins in every file, with no warning

**Kind:** silent. **Where:** `js_libs` in [src/interp.rs](../../src/interp.rs) (about 2187) is keyed by the name; [src/check.rs](../../src/check.rs) (about 2063) never compares the sources.

```quillo
// a.qwe
client /m1.mjs use helper
component A() { state s = ""
    <p id="a" @mount={(el) => { s = helper() }}>{s}</p> }
// b.qwe
client /m2.mjs use helper
component B() { state s = ""
    <p id="b" @mount={(el) => { s = helper() }}>{s}</p> }
// main.qwe
import a.qwe use A
import b.qwe use B
route GET /:f = file(f)
route GET / = <html><body><A/><B/></body></html>
```

Chrome shows `{"a":"m2","b":"m2"}`, and `/m1.mjs` is never fetched; `check` says ok. Two such lines in one file are also accepted, while for a `.qwe` import the same is an error.

**Suggestion:** keep browser names per file, as `.qwe` names are, or an error where two sources give one name.

### Q-809. NaN and ±inf reach Rust as null

**Kind:** silent. **Where:** arguments go through `Value::to_json` ([src/native.rs](../../src/native.rs) about 1233), and [src/value.rs](../../src/value.rs) (about 395) makes a non-finite float `Null`.

```quillo
// n.rs
//! effects: none
pub fn seen(x: Option<f64>) -> String { format!("{:?}", x) }
pub fn dump(v: serde_json::Value) -> String { v.to_string() }
// main.qwe
use n.rs
let nan = 0.0 / 0.0
print(seen(nan), seen(1.0 / 0.0), dump([nan, 2.5]))   // None None [null,2.5]
```

A plain `f64` parameter is told "must be float, got null".

**Suggestion:** pass f64 values as they are, and refuse NaN for a `serde_json::Value` by the parameter's name, as an i32 overflow is.

### Q-810. An error value reaches Rust as an ordinary map

**Kind:** silent. **Where:** [src/value.rs](../../src/value.rs) (about 402: an error becomes `{"error": ...}`), used by [src/native.rs](../../src/native.rs) (about 1233).

With `pub fn store(v: serde_json::Value) -> String { format!("stored {}", v) }`, `store(json.parse("{oops"))` prints `stored {"error":"invalid JSON: key must be a string..."}`, and `check` says ok. A typed parameter gets "must be int, got map", which loses the reason.

**Suggestion:** an error value given to a Rust function gives that error back, as builtins do.

### Q-811. A raw identifier (`r#type`) in a Rust file hides every `pub fn` after it

**Kind:** silent. **Where:** [src/native.rs](../../src/native.rs) (about 146, and `block_end` about 302) take `r#` for the start of a raw string without needing a `"`.

```rust
//! effects: none
pub fn kind(r#type: i64) -> i64 { r#type }
pub fn later_one() -> i64 { 5 }
```

`check` gives no warning for the file; `check` and `run` then say "unknown name later_one".

**Suggestion:** a raw string is `r#"` (or `r"`); `r#` followed by a letter is an identifier.

### Q-812. `x.rs` and `x.qwer` in one folder delete each other's library

**Kind:** loud. **Where:** `stem()` in [src/native.rs](../../src/native.rs) (about 728), the clean-up in `keep_lib` (about 1003) and `lock_built` (about 913).

With both `use x.rs` and `use x.qwer`, `quillo build` prints "built" for two libraries, but only one stays in `.quillo/native`, and `plugins.lock` has one line; `run --no-build` then fails with "./x.qwer is not built", and a plain run builds both again at every start. `a-b.rs` with `a_b.rs` does the same, and so would `Img.rs` with `img.rs` on Linux.

**Suggestion:** a library name from the whole file name (extension included, case kept), or an error naming the two files.

### Q-813. Rust that `check` accepts but the glue cannot build loses the whole file

**Kind:** loud. **Where:** `quillo_type` in [src/native.rs](../../src/native.rs) (about 374) takes a nested `&str`, which the runtime has no `FromQ` for; `parse` ignores attributes on a `pub fn`.

`pub fn greet(name: Option<&str>) -> String` (or `Vec<&str>`), and `#[cfg(target_os = "linux")] pub fn linux_only() -> i64 { 2 }` on macOS: `check` says ok; the build fails with "lib.rs:246:26: the trait bound `&str: FromQ` is not satisfied" and "lib.rs:249: cannot find function `linux_only` in module `user`", errors in Quillo's own glue file.

**Suggestion:** refuse `Option<&str>` and `Vec<&str>` in `check` ("use `Option<String>`"), and copy a `pub fn`'s `#[cfg(...)]` onto its glue.

### Q-814. A `//` line before the `//!` header makes Quillo ignore the header

**Kind:** loud. **Where:** [src/native.rs](../../src/native.rs) (about 87 and 88) stops at the first line that is not `//!`.

```rust
// SPDX-License-Identifier: MIT
//! effects: none
pub fn twice(n: i64) -> i64 { n * 2 }
```

Called in `fn calc(n: int) with time`, it is refused: "file does not say what it does ... put `//! effects:` ... at the top of its .qwer file"; the line is there, and the file is a `.rs`. With a `//! [dependencies]` line after a comment, cargo stops at "unresolved crate `itoa`", with no hint why. `#![...]` before the header does the same.

**Suggestion:** skip `//` comments, blank lines and `#![...]` before the header; name the file's real extension in the message.

### Q-815. Common Rust signatures are refused

**Kind:** loud (`check` warns). **Where:** `quillo_type` in [src/native.rs](../../src/native.rs) matches `Option<`, `Result<` and `Vec<` on the whole text; `split_top` keeps comments.

`std::io::Result<String>`, `std::vec::Vec<i64>`, `std::option::Option<i64>` and `std::result::Result<...>` "cannot cross"; `-> &'static str` is "`'static str` cannot cross"; a trailing `// in cents` on a parameter's line is "cannot read the parameter `// in cents`"; `a: i64 /* left */` is "`i64 /* left */` cannot cross".

**Suggestion:** strip comments, and the `std::` paths of the types Quillo knows, before reading a signature; take `&'static str` as `&str` for a return.

### Q-816. A Rust alias is the program's, not the file's, and the fix the warning gives does not parse

**Kind:** silent. **Where:** [src/check.rs](../../src/check.rs) (about 2121 and 2130).

`import a.rs use who as name` in main.qwe, then `use other.qwe`, which has `import b.rs use who as name`: `name()` was `a.rs`'s and becomes `b.rs`'s for the whole program. The warning says "take one under another name: `import b.rs use name as b.rs_name`", which does not parse (`.rs` kept) and names the alias, not the export (`who`). The builtin warning has the same fault: `import lib/my-tools.rs use len as my-tools.rs_len` is "expected end of statement, found `-`". A library's own `fn imei_valid`, which never takes the plugin's, is "defined twice" with the plugin's.

**Suggestion:** Rust names are per file, as `.qwe` names are; the suggested fix uses the export's name and a valid identifier.

### Q-817. A main file run through a symlink looks for its used files beside the link

**Kind:** loud. **Where:** the interpreter's `base_dir` and `modules::resolve_from` take the link's folder.

`opt/tool/tool.qwe` with `use lib/a.qwe` and `use ./lib/b.qwe`, and `bin/tool.qwe -> ../opt/tool/tool.qwe`: `quillo run bin/tool.qwe` says "cannot read lib/a.qwe ... cannot read lib/b.qwe ... unknown name a, b"; run directly, it works. It breaks a `#!/usr/bin/env quillo` tool linked into the PATH.

**Suggestion:** resolve the main file's real path first, as scripts and Python do.

### Q-818. A `fn` after a top-level `return` in a used file passes `check`, and the program does not start

**Kind:** loud. **Where:** [src/interp.rs](../../src/interp.rs) (about 2270, `Ctrl::Return => break`), while `check` defines every `fn`.

```quillo
// lib.qwe
fn early() = 1
return
fn late() = 2
// main.qwe
use lib.qwe
print(early(), late())
```

`check`: "ok (2 functions)". `run`: "main.qwe:2: unknown name late".

**Suggestion:** define a file's `fn`s before running its statements, as `check` assumes, or have `check` say that code after a top-level `return` never runs.

### Q-819. Q-523's fix covers only `import x.qwe use name`

**Kind:** loud. **Where:** `resolve` in [src/modules.rs](../../src/modules.rs) (about 113), and `check`'s path for a missing file.

A missing file taken whole (`use lib/b.qwe`, under `--watch` too) still cascades to "unknown name b". A missing `.rs` or `.qwer` is "cannot read ./imagin.rs", with no near file named, and the cascade. `import imaging use img` with only `imaging.rs` looks for `./imaging.qwer`. `use helpers.rs` with no `.rs` silently loads `helpers.qwe`.

**Suggestion:** Q-523's single message at the import line for every form, the near-file suggestion for every extension, and no switching of extension.

### Q-820. Upper-case extensions

**Kind:** loud. **Where:** `is_rust` in [src/native.rs](../../src/native.rs) (about 49) matches case; `is_script` ([src/ast.rs](../../src/ast.rs) about 575) does not.

`use Lib.QWE` (unquoted) is "cannot read Lib.QWE.qwe", while `use "Lib.QWE"` works. `use "Tools.RS"` reads the Rust file as Quillo (Q-453, in upper case).

**Suggestion:** compare extensions without case, in both functions.

### Q-821. Types and `as`

**Kind:** silent. **Where:** [src/modules.rs](../../src/modules.rs) (about 236).

`import lib.qwe use Point as P` is accepted, and `P({ x: 3 })` makes a Point, but `fn norm(p: P)` is "unknown type `P`". `import lib.qwe use one as Point` silently replaces the program-wide type's constructor in that file: `Point({ x: 1 })` is "`one` takes 0 arguments".

**Suggestion:** an alias of a type is a type; `as` cannot take a name that is a type of the program.

### Q-822. Smaller messages from native modules and imports

**Kind:** loud.

- "must be a list of !does not fit in u8: 256 (item 1)": the `!` marker leaks into the text.
- "NaN is not a number Quillo has", and the error for a u64 answer too large, do not name the function.
- The "taken a second time" warning names a used file by its absolute path; other messages use `./lib/a.qwe`.
- Every failed build of a file whose functions take no parameters shows the glue's "lib.rs:243: unused variable `a`".
- A straight chain of 65 files is "files use each other more than 64 deep" (no cycle).
- `client "javascript:..."` is told it "is Quillo: import javascript:... use"; `client "x\".js" use L` suggests `use use L`.
- A browser name used in a file that did not take it gets no warning.
- `pub fn view()` in Rust is exported and cannot be called ("unexpected keyword view").

## Dates, i18n, images, text

Compared with Node's `Intl` (ICU 77), Python's `zoneinfo` and `ffprobe`. Server and browser were both run (runtime.js loaded in Node).

### Q-823. Fractional plural counts pick the wrong CLDR form in about 20 locales

**Kind:** silent. Reproduced for this report. **Where:** `plural_of` in [src/i18n.rs](../../src/i18n.rs) (its early `if frac || ...` return), and `plural` in runtime.js (about 1878).

```quillo
translations({ fr: { kg: { one: "{count} kilo", other: "{count} kilos" } } })
print(with_locale("fr", () => t("kg", { count: 1.5 })))   // 1.5 kilos  (CLDR: one, "1.5 kilo")
print(with_locale("fr", () => t("kg", { count: 0.5 })))   // 0.5 kilos  (CLDR: one)
```

A grid of 41 locales and 51 counts (2,091 comparisons with `Intl.PluralRules`): fractions are wrong in fr, pt, hi and he (0.5 is `one`), sl (every fraction is `few`), hr, sr and bs (1.1 is `one`, 1.2 `few`), lv, is, mk and da. A count written with ".0" is wrong in es, ar, cy, ga, mt, be, lv, is, tr, el, hu, nb and pa (`es` "1.0" is `one`, `ar` "2.0" is `two`), and in lt "1.0" gives `many` where CLDR gives `one`. fil and tl fall back to English, so 2, 3, 5, 7 and 10 get `other`, where CLDR gives `one`. Server and browser give the same wrong answers. The guide says the rules are CLDR's, and also that "a fraction takes ... `other` elsewhere".

**Suggestion:** CLDR's operands (`n`, `i`, `v`, `f`, `t`) for fractions and visible zeros, with the rules for those locales; and the guide's sentence made to agree.

### Q-824. Two `image.convert` calls to one target at once share a temp file

**Kind:** silent. **Where:** `make` in [src/imaging.rs](../../src/imaging.rs): the temp file is `<name>.tmp<pid>`.

Route `/a` converts a 3000x3000 PNG to `out/mix.png`, route `/b` converts a 400x200 one to the same file, 20 ms later: `/a` answers `{"error":"internal error"}` (the log: "image: No such file or directory"), `/b` answers `{"width":400,"height":200}`, and the file holds the 3000x3000 image. Four identical requests at once: two or three fail (6 runs). Lazy thumbnails and double uploads do this.

**Suggestion:** a temp name unique per call (a counter, or `tempfile` in the target's folder).

### Q-825. `image.convert` and `image.info` ignore EXIF orientation

**Kind:** silent. **Where:** `make` and `info` in [src/imaging.rs](../../src/imaging.rs). The `image` crate 0.25 has `ImageDecoder::orientation()` and `DynamicImage::apply_orientation`.

A 400x200 JPEG with EXIF orientation 6 (shown as 200x400): `image.info` says 400 by 200; `convert(..., { width: 100 })` gives 100 by 50, the pixels not turned, and no EXIF in the output, so a browser cannot turn it either. Phone photos come out on their side.

**Suggestion:** apply the orientation on reading, in `info` and `convert`.

### Q-826. Transparent pixels turn black in JPEG output

**Kind:** silent. **Where:** [src/imaging.rs](../../src/imaging.rs): `img.to_rgb8()` drops the alpha.

An RGBA PNG with a transparent background converted to `.jpg`: the corner is (0, 0, 0).

**Suggestion:** composite onto white (or a `background` option) before writing a format without alpha, or say so in the guide.

### Q-827. In the browser, `format_number` of a large whole float writes its binary digits

**Kind:** silent. **Where:** runtime.js (about 2620): `isInt(x)` is true for any whole float, which then goes through `BigInt(x)`.

`format_number(6.022e23, "en-US", 0)` is "602,200,000,000,000,000,000,000" on the server and in `Intl`, and "602,200,000,000,000,027,262,976" in the browser; 1e23 is "99,999,999,999,999,991,611,392". Past 7.9e28 the browser groups the digits, and the server writes the number plain.

**Suggestion:** a float past 2^53 goes through its shortest decimal text (`decFromFloat`), not `BigInt`.

### Q-828. `date.format` styles for regional locales are not CLDR's

**Kind:** silent. **Where:** `LOCALES` and `names()` in [src/dates.rs](../../src/dates.rs), and `dtEn` in runtime.js. The guide says the styles are "CLDR's forms".

| `date.format(d, { locale, style })` | Quillo | `Intl` |
|---|---|---|
| `"2026-09-05"`, en-CA, short | 9/5/26 | 2026-09-05 |
| fr-CA, short | 05/09/2026 | 2026-09-05 |
| pt-PT, medium | 5 de set. de 2026 | 05/09/2026 |
| en-ZA, short | 05/09/2026 | 2026/09/05 |
| en-AU, short | 05/09/2026 | 5/9/26 |
| `"2026-01-05"`, de-AT, long | 5. Januar 2026 | 5. Jänner 2026 |

Also en-IN short, en-IE full (a comma), fr-CH and it-CH short (dots), nl-BE and es-MX: 26 of 225 cases outside Japanese.

**Suggestion:** the regional patterns from CLDR's data for the locales in the table, on both sides.

### Q-829. Server and browser disagree on which time zones exist

**Kind:** silent. **Where:** `zone()` in [src/dates.rs](../../src/dates.rs) against `dtZone` in runtime.js.

`date.valid_zone("+0530")` is false on the server and true in the browser, and `date.to_zone(t, "+0530")` is an error on the server and "...17:30:00+05:30" in the browser; "+05" the same. `date.valid_zone("Etc/Unknown")` and `"Factory"` are true on the server (jiff's UTC-like names) and false in the browser; Python refuses `Etc/Unknown` too. A zone the server took in `set_zone` can stop a component. `date.parse` reads a "+0530" offset, but no zone argument takes it.

**Suggestion:** one list on both sides: offsets as zones on both (or neither), and `Etc/Unknown` and `Factory` refused.

### Q-830. A huge service `timeout` or `cooldown` panics as the program loads

**Kind:** loud. Reproduced for this report. **Where:** [src/service.rs](../../src/service.rs) (about 367 to 382, `Duration::from_secs_f64`).

```quillo
service billing = "billing.qwe" { timeout: 1e300 }
```

"panicked at .../core/src/time.rs:962:23: cannot convert float seconds to Duration: value is either too big or NaN". The same for `math.inf` and `"99999999999999999999s"`, and for `cooldown`.

**Suggestion:** `Duration::try_from_secs_f64`, and the "is a duration" error that `"1.5h"` gets.

### Q-831. `markdown_page` takes its title from a `#` line inside a code block, and none from a setext heading

**Kind:** silent. **Where:** [src/builtins.rs](../../src/builtins.rs) (about 2364 to 2366: `text.lines().find_map(strip_prefix("# "))`).

```quillo
print(markdown_page("```sh\n# install the tools\nnpm i\n```\n\n# Getting started\n"))   // <title>install the tools</title>
print(markdown_page("Setext Title\n============\n\ntext"))                              // <title></title>
```

An indented code line or `<!-- # hidden -->` does the same; `# Title #` gives "Title #"; `#\tTabbed` gives no title; `# *Emph* and `code`` keeps the marks (escaped, so safe).

**Suggestion:** the first level-1 heading's text from the parser's events.

### Q-832. A whole float `count` reads "1.0 item" on the server and "1 item" in the browser

**Kind:** silent. **Where:** `count_of` in [src/i18n.rs](../../src/i18n.rs), `pluralCount` in runtime.js.

`t("items", { count: 1.0 })` is "1.0 item" on the server (it shows "1.0", and picks `one` where CLDR's English gives `other`) and "1 item" in the browser; `1.0m` gives "1.0 items". `count: "0x1"` gives `one` in the browser (`Number("0x1")` is 1) and `other` on the server.

### Q-833. `Accept-Language` reads the `q` weight only in lower case

**Kind:** silent. **Where:** [src/i18n.rs](../../src/i18n.rs) (about 236, `strip_prefix("q=")`). With locales fr and de, `fr;Q=0.1, de;q=0.9` serves fr; with `q=0.1` it serves de. Parameter names are case-insensitive (RFC 9110).

### Q-834. The highlighter mis-colours multi-line strings and some quotes, and colours a language it does not know

**Kind:** docs, cosmetic. **Where:** [src/highlight.rs](../../src/highlight.rs); server and browser agree token for token.

A Python `"""docstring"""` over several lines colours its inner lines as code (`for`, `and` as keywords); multi-line Rust strings the same. Rust `'"'`, JavaScript `/"/` and the shell's `it's` make the rest of the line a string; the shell's `${#arr[@]}` starts a comment; `0..n` is one number. `highlight("print(1)", "nope")` gives `hl-f` and `hl-n` spans, where the guide says "anything else as escaped text". The fence info `rust,ignore` is not read as Rust.

Not counted: a pt-BR key never falls back to pt (documented); `[^1]: Note.` becomes a link, since footnotes are off; a byte-order mark in a locale file is "invalid JSON ... column 1" without naming it; `format_number` with no digits writes every digit (1.23456, where `Intl` gives 1.235); -0.001 to two digits is "0.00" (`Intl` and Python: "-0.00"); Havana in November 2026 and Chisinau in March 2026 differ between macOS's tz data and Node's ICU.

## The command line, `.env`, the language server

Run in a scratch folder; `quillo install` was tried with a local registry and argument errors only.

### Q-835. A `.env` that is not UTF-8, or not readable, is skipped without a word

**Kind:** silent. Reproduced for this report. **Where:** [src/dotenv.rs](../../src/dotenv.rs) (about 39 to 43: `Err(_) if !required => return Ok(())`).

```sh
printf 'KEY=s3cr\xe9t\nOTHER=2\n' > .env
printf 'print(env("OTHER"))\n' > main.qwe
quillo run main.qwe        # null, exit 0, nothing printed
```

A UTF-16 file from Notepad and `chmod 000 .env` do the same: every `env()` is null. With `--env-file .env` it says "cannot read .env: stream did not contain valid UTF-8" and exits 2.

**Suggestion:** only a missing `.env` is silent; any other error is said, and stops the start as `--env-file` does.

### Q-836. `--watch` started while a used file is broken never reloads when that file is fixed

**Kind:** silent. **Where:** `watched_files()` in [src/server.rs](../../src/server.rs) (about 2289) takes only the main file when the load failed, or the files of the version before.

`main.qwe` has `import lib.qwe use ver`, `lib.qwe` has `1 +`. Start `quillo run --watch`, then fix `lib.qwe`: the page stays "The program did not load" (500) 2 and 3 s later, and reloads only when `main.qwe` is saved. A failed reload that adds `import b.qwe` (b missing) does the same: making `b.qwe` never reloads.

**Suggestion:** watch every file the failed load named (read, or tried), and the folders of missing ones.

### Q-837. `quillo fix` writes its import before a file's byte-order mark

**Kind:** silent: the tool breaks a working program. **Where:** `fix()` and `with_use()` in [src/modules.rs](../../src/modules.rs) keep the BOM in the text, so line 1 is not seen as a comment or an import.

`ui.qwe` is a BOM and `// ui\r\nfn page() {\r\n  str(ver())\r\n}`; `check` passes with a missing-import warning. `fix` writes `import lib.qwe use ver\n\n` before the BOM, and the next `check` fails: "ui.qwe:3: error: unexpected character U+FEFF (a byte-order mark)". The same when the file already starts with an `import`. The line added is LF in a CRLF file.

**Suggestion:** keep the BOM first, and write the file's own line ends.

### Q-838. `.env`: `KEY= # note` sets "# note"

**Kind:** silent. Reproduced for this report. **Where:** [src/dotenv.rs](../../src/dotenv.rs): `v.trim_start()` runs before `split(" #")`.

The guide (USERGUIDE.md line 158) says "` #` ends a bare value". `KEY= # note` is "# note", `KEY=   # set me` is "# set me", and `C=<tab>#x` is "#x".

**Suggestion:** a value that starts with `#` after the spaces is empty.

### Q-839. `.env`: an unclosed quote takes the rest of the file, while saying the line was skipped

**Kind:** silent. **Where:** `parse_noting` in [src/dotenv.rs](../../src/dotenv.rs) keeps the value after noting the line as skipped.

`KEY='abc` then `OTHER=2`: Quillo prints ".env:1: not KEY=value, skipped", yet `KEY` is "abc\nOTHER=2" and `OTHER` is null.

**Suggestion:** skip the line as it says, and go on with the next.

### Q-840. A NUL in a `.env` value panics

**Kind:** loud. **Where:** `set_var` in [src/dotenv.rs](../../src/dotenv.rs). `printf 'KEY=x\0\n' > .env` gives "panicked at .../env.rs:362: failed to set environment variable "KEY" ... NUL byte", exit 1.

**Suggestion:** ".env:1: a value cannot hold a NUL byte", and skip it.

### Q-841. `--reload` with `--instances` never reloads, and nothing says so

**Kind:** silent. **Where:** `run_instances` in [src/main.rs](../../src/main.rs) checks only `o.watch`; `serve_many` in [src/server.rs](../../src/server.rs) starts no watcher.

`quillo run main.qwe --reload --instances 3`, then an edit: still `v=1` from every instance. `--reload` alone gives `v=2`; `--watch` with `--instances` is refused with a message.

**Suggestion:** reload each instance, or refuse the pair as `--watch` is refused.

### Q-842. `--split --reload` never takes a change to a service's file, though the log says it reloaded

**Kind:** silent. **Where:** [src/main.rs](../../src/main.rs) gives only `o.watch` to `service::split`.

`quillo run app.qwe --split --reload` with `service billing = "billing.qwe"`, then an edit to `billing.qwe`: the log says "quillo: reloaded app.qwe (1 routes)", and the service still answers "billing v1". With `--watch` it answers "billing v2".

**Suggestion:** pass `--reload` to the split services as `--watch` is.

### Q-843. `--split --port 0` tells every service the app is at port 0

**Kind:** silent. **Where:** `app_url` in [src/main.rs](../../src/main.rs) is built from the `--port` text, not the port bound.

Each service is started with `QUILLO_APP_URL=http://127.0.0.1:0` (seen with `ps eww`), while the app listened on 57948. A service's `channel.broadcast`, `send` and `peers` go to that address, and their errors are dropped ([src/wt.rs](../../src/wt.rs) about 1009 and 1038, giving 0, false and `[]`), so Q-087's relay quietly stops (the address seen; the effect read from the code).

**Suggestion:** the URL of the port the app is bound to.

### Q-844. Language server: a project opened through a symlinked path gets no diagnostics, and unsaved text is ignored

**Kind:** silent. **Where:** `update()` in [src/lsp.rs](../../src/lsp.rs): the `d.file == file` comparison and the `open` map use the link's path, while the checker and `path_to_uri` canonicalize.

`lib.qwe` (used by `main.qwe`) opened as `file:///tmp/.../proglink/lib.qwe` (on macOS `/tmp` is itself a link): that URI gets `[]`; the error goes to `file:///private/tmp/.../lib.qwe`, and it is the error in the file on disk, not in the editor's text.

**Suggestion:** map canonical paths back to the URI the client opened, and key open documents by canonical path.

### Q-845. Language server: a `content-length` header in another case hangs that request and every one after it

**Kind:** loud. **Where:** `read_message` in [src/lsp.rs](../../src/lsp.rs) reads only `Content-Length:`. With `content-length: N` or `CONTENT-LENGTH:`, the body is read as headers, and nothing after it is answered. Header names are case-insensitive.

### Q-846. `quillo install --dir --force demo` installs into a folder named `--force`

**Kind:** silent. **Where:** `value()` in `parse_args`, [src/install.rs](../../src/install.rs): Q-381's fix was not applied there.

`quillo install --dir --force demo` says "installed demo 1.2.0 into --force/demo.qwer", makes the folder and applies no force; `--dir --update demo` the same. `--registry --list` is "--list/plugins/index.json: Bad URL".

**Suggestion:** a value that starts with `--` is refused, as `run` does since Q-381.

### Q-847. `quillo serve` parses its options its own way

**Kind:** loud. **Where:** `serve()` in [src/main.rs](../../src/main.rs) has its own copy, without Q-070's and Q-381's fixes.

`quillo serve . --port=18401` is "unknown option `--port=18401` (did you mean `--host`?)", exit 2, though the usage says `=` works; `--workers=4` and `--log=json` the same. `quillo serve . --with --watch` says "started '--watch' beside the program".

**Suggestion:** use `program_and_flags`, as `run` does.

### Q-848. The REPL fed from a pipe stops at a line that is not UTF-8, with exit 0

**Kind:** silent. **Where:** [src/main.rs](../../src/main.rs) (about 1484): `read_line(...).unwrap_or(0) == 0` takes an error for the end.

`printf 'print("one")\nprint("\xe9")\nprint("three")\n' | quillo repl` prints `one` only, and exits 0; `-i` the same.

**Suggestion:** report the bad line (and go on, or exit 1).

### Q-849. A policy file takes limits of 0 that the command line refuses

**Kind:** loud. **Where:** `add()` in [src/policy.rs](../../src/policy.rs).

`[limits] max_body = 0` answers every POST 413 (nginx reads 0 as no limit), while `--max-body 0` is refused. `max_compute = "0s"` ends requests after about 65 ms with "this request computed for longer than --max-compute allows", though no `--max-compute` was given: the limit came from the policy.

**Suggestion:** the command line's range checks in the policy, and messages that name the policy file when the limit came from it.

### Q-850. `quillo new` into an existing empty folder needs write permission on its parent

**Kind:** loud. **Where:** `make()` in [src/new.rs](../../src/new.rs) makes its temp folder beside the target.

`cd locked/empty && quillo new .`, where `empty` is writable and `locked` is not: "cannot make .../locked/.empty.quillo-new-PID: Permission denied", exit 1.

**Suggestion:** when the target exists and is empty, build inside it.

### Q-851. `--help` and leading options

**Kind:** loud. **Where:** `real_main` in [src/main.rs](../../src/main.rs) (the `Some(_) => usage()` arm; `-h` is read only inside `run_options`).

- `quillo run -h` and `quillo fcgi --help` in a folder without `main.qwe` say "no program: name a .qwe file ...", exit 2; with a `main.qwe` they print the usage.
- `quillo lsp --help`, `repl --help` and `dap --help` start the server or the REPL and wait on stdin.
- `install -h` prints its usage to stderr, `new -h` to stdout.
- `quillo --watch main.qwe` and `quillo --port 0 srv.qwe` print the whole 131-line usage, exit 2, naming nothing. The usage says options may stand anywhere; Q-389 covered unknown commands only.

**Suggestion:** `-h` and `--help` read first, for every command, to stdout; an option before the command taken as `run`'s, or one line saying "put the command first: `quillo run --watch main.qwe`".

### Q-852. Smaller ones

**Kind:** loud.

- The unknown-command suggestions leave out `serve`, `classes` and `fcgi`: `quillo fcg` gets "did you mean `fix`?", and `serv` and `classe` get nothing (the `COMMANDS` list in `real_main` is short).
- Under `--watch`, each failed reload prints every error twice ("./lib.qwe:3: error: ..." and then "quillo: ./lib.qwe:3: error: ..."); a good one says "reloaded main.qwe (1 routes)" ([src/server.rs](../../src/server.rs) about 2225 to 2267).
- A policy file starting with a BOM is "line 1: expected 'key = value' or '[section]'", exit 2; `.qwe` and `.env` files skip the BOM (Q-386, Q-380).
- `--env-file` is missing from `quillo help`, though `run` takes it and the guide documents it.
- `--tls-cert` given twice: the first wins, and only the first is checked (a `OnceLock`), while `--port` takes the last; `--tls-cert /nonexistent --tls-cert /etc/hosts` is refused.
- `quillo classes` and `build` on a file that is not UTF-8 say "stream did not contain valid UTF-8", with no line or column, unlike `check` and `run`.
- The language server answers requests before `initialize` and after `shutdown` as usual (the protocol asks for -32002 and InvalidRequest), and a body that is not JSON gets no -32700.
- `quillo repl hello.qwe` and `quillo repl --bogus` ignore their arguments.

## What behaves well

Counts are cases. Most of the sweep is here: about 6,900 of its 7,500 cases gave the right answer.

| Area | What held |
|---|---|
| `files()` and paths | Traversal in every form tried (`..`, `%2e%2e`, `%252e`, `..%5c`, NUL, absolute, `~`, U+2024/FF0E/FF0F/2215, overlong UTF-8): 27. Dot files, `.git`, `.ENV` on read, `.qwe` in any case, links out, dangling and looping: about 100. `read_file`, `file()`, `write_file`, `delete_file`, `image.convert`, `save_upload` and upload names: about 160. `quillo serve`'s raw request targets: 23 |
| Markup and URLs | `javascript:` and `vbscript:` in every case, with spaces, tabs, control characters, in 14 attributes, from templates, `raw()` and Markdown: 30. Spread keys (`on*`, `bind:on*`, `@click`, `srcdoc` in any case, `__proto__`, odd characters): about 60. Server Markdown with links, entities, HTML blocks and fences: about 30, plus 104 fences. Sandboxed `srcdoc`, `object`, `embed` and `iframe` in Chrome: 59, no payload ran |
| HTTP | MIME types: 25. Ranges and conditionals apart from Q-723: about 45. gzip negotiation: 25. Status codes 0, 1xx, 600 refused: 30. CR, LF, NUL and DEL in headers refused by `respond`: 29. `redirect` refusing `javascript:`: 26. Cookie names and values, 3,000 cookies, a 60 KB header, 431 at 70 KB: about 55. Query and form parsing, 100,000 fields: about 55. Multipart forms: about 50. HEAD, OPTIONS and cross-site checks: 21. SSE framing: 17. A 50 MB answer: fast |
| Databases | Ints at every edge on all three: about 45. PostgreSQL floats and numerics: about 35. Text, NUL and 40 MB: about 30. Dates, times, intervals and infinities: about 70. Arrays: about 40. uuid, enums, domains: about 20. MariaDB's types: about 70. Placeholders in strings, comments, quoted names, dollar quotes, PostgreSQL's keywords: about 120. `exec_many`, 70,000 rows, all or none: about 90. Transactions and upserts: about 65. Connection URLs: about 60 |
| Components | Keyed lists reversed, inserted, swapped, emptied and refilled; inputs follow their rows: about 30. 10,000 rows filtered on each key: 5 to 90 ms. Checkboxes, radios, dates, files, `contenteditable`, typing before the runtime starts: 26. Events and modifiers: 15. Timers and effects stopped with their component, out-of-order `live()` answers: 11. SVG: 12. `<details>`, `<dialog>`: 10. Back and forward, scroll restored: 33 |
| Server and browser | 1,673 expressions the same: number text, 64-bit int arithmetic, `round` at .5, `int()` and `float()` of odd text, all 27 `bit.*` cases, Unicode `len`, slicing, case, `normalize`; 159 of 161 regexes; 212 of 217 dates across six time-zone pairs; 145 of 146 `format_number` cases over about 140 locales |
| Core values | Unicode case mapping: 34. Indexing and slicing at every edge: 40. Number parsing: 45. Arithmetic and overflow: 40. The JIT against `QUILLO_JIT=0`: 60, all the same. Lists, maps, records, patterns, error values: about 180. JSON, 500 levels deep: 58. Regexes: 45. `math` against Python's domain and range errors: 60 |
| Concurrency | `+=`, `push` and `set` on top-level state from 400 requests at once: exact. `parallel` of 100,000 functions: 1.06 s. 50 pages and 1,000 publishes: two frames a page, all at the final value. Viewer isolation behind `--trust-proxy`: 0 wrong in 90. Channels: 6,000 of 6,000 in order; 140 peers coming and going. SSE resuming from `Last-Event-ID`. Ticks that skip rather than overlap, three servers sharing one job |
| Modules and Rust | Cycles, diamonds, one file by many paths (loaded once), routes across files: about 30. Import syntax: 22. Rust values at their edges (i32 overflow named, NUL text, `Vec<Option>`, nested maps, panics as a 500 the server survives, 50 MB strings, 20 parameters): 28. Failed builds under `--watch` keep the old version: 8 |
| Dates and text | ISO 8601 parsing: 74. `date.at` across DST in 13 zones, both options: 142, all equal to Python's `zoneinfo`. Histories and aliases: about 85. Format tokens: 70. Integer plurals in 41 locales: all equal to `Intl`. Interpolation: about 60. `Accept-Language`: 44. `format_number` against `Intl`: about 600. Markdown, 27 pathological inputs each under 160 ms. Images: a 50000x50000 bomb refused at its header, truncated and corrupt files, CMYK |
| Tools | Option values for `run`: 13. Signals during a request: 5. `--watch` through 50 saves a second, renames and deletions: 9. `check` and `run` on UTF-16, binary, NUL, BOM and CRLF files: 20. `.env` forms: 12. The language server's UTF-16 columns with emoji, a 60,000-line file in 0.07 s: 12 |

## Where the AI struggled

| What happened | Why | Cost |
|---|---|---|
| The first sweep (2026-11-03, on daa8c35) ran nine agents for half an hour and stopped with its session; only the database agent had reported | The session ended while they ran | The sweep was run again two days later on 0.7.0, with the database agent re-checking its 15 findings first |
| That sweep's HTTP agent left ten `quillo run` servers running for two days | It recorded no PIDs and was stopped before cleaning up | Found and stopped by PID before this sweep; every agent here wrote its PIDs to a file and stopped them, which held |
| The security agent stopped after about 110 cases | A safety classifier cut one of its responses off in the middle of the Markdown batch | A follow-up agent did the parts it had not reached (files and paths, `srcdoc`, highlighting) with 576 cases, and found 5 of this report's security issues |
| The modules agent's clean-up ran `kill 0` | A blank line in its PID file | It killed only that clean-up shell; no other process was touched |
| Totals in the first draft of this report were wrong (61 silent, 17 security) | Counted by hand before the summary was done | Counted from the summary table instead |
| A first count of `grep -c alice` on the served `-wal` printed nothing | The pipeline's output was lost | Counted with Python: 3 and 3 |

## Order of work, and questions

1. The security issues, Q-700 to Q-710, with Q-796 and Q-773. Q-700 and Q-707 get around FEEDBACK51's fixes for Q-667 and Q-668; Q-704 and Q-705 around Q-361's; Q-708 around Q-581's.
2. Data that is lost or wrong without a word: Q-727 to Q-729, Q-797, Q-798, Q-807, Q-835, Q-757 to Q-761.
3. Q-746, a regression of FEEDBACK44's work, before more programs meet it.
4. The server and browser differences, Q-772 to Q-782: one decision (below) settles most of them.
5. The rest by area: each area's issues share their code.

Decided on 2026-11-05:

- **Q-775 and Q-776:** numbers must be accurate and secure, money above all: the browser gives the server's answer for every number, or an error, never a different number. The type-keeping wrapper and exact big ints, not a note in the guide.
- **Q-800:** the latest call only.
- **Q-736:** each database's driver handles its database's quirks itself, so that one database's limitation never reaches developers as Quillo's. For Q-736, the MySQL driver refuses a database that is not there, as PostgreSQL's does, and creates one only for a tenant. The other differences between drivers (Q-735, Q-739 to Q-741, Q-747, Q-748, Q-751 to Q-753) follow the same rule.

- **Q-801:** the `:*` wildcard is meant for programs (other apps use it): it stays, and the guide documents it, with a warning about topics built from a visitor's text. The comma, the length and the empty topic are still to fix.

No question is left open.

## Probe scripts

In /private/tmp/claude-501/-Users-restiguay-dev-quillo-app1/83c0252c-4dff-4fce-b38c-b0978e42215f/scratchpad/f55/ (this session's scratch, not kept), one folder per area:

| Folder | What is there |
|---|---|
| http/rep/ | main.qwe and run.sh: every HTTP repro |
| sec/, sec2/ | the markup batches (b1 to b3), the browser servers and their drivers; the path, `srcdoc` and highlighting probes |
| db/min/, db/old/ | one .qwe per database issue; the stopped sweep's repros, run again |
| dom/ | pages a/ to mm/ and their Chrome drivers |
| parity/ | batches b1 to b13, the page with one island per case (r2/r.json is its result), the form post (r5/drive.mjs), the float ties (ties/) |
| core/ | final.qwe, every core repro in one file; the batches |
| conc/ | lib.mjs (a live and WebSocket client), chan.mjs (a channel peer), programs t1 to t23 |
| mod/min/ | m1 to m8, each with its files |
| text/ | rt.mjs and cmp.sh (runtime.js in Node, compared with the server), pl.qwe and pl.mjs (the plural grid), img/ (the test images and race.qwe) |
| cli/ | the command-line probes |
| notes/ | each agent's findings, as received |
| verify/ | the cases reproduced again for this report |
