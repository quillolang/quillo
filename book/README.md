# Programming Quillo: the manuscript

The book, in Markdown, one file per chapter in `chapters/`, read in the order of their
numbers. `metadata.yaml` holds the title, subtitle and author for the build.

About 74,000 words: 271 pages at 6×9 in, before the contents and an index.

## Where the chapters come from

| Files | Source |
|---|---|
| `00-copyright`, `01-preface`, `*-part-*`, `71-appendix-examples`, `90-about-the-author` | Written for the book |
| `11-introduction` | README.md: the opening, Features and Performance |
| `13`, `14`, `21`, `22`, `31`, `53`, `62`–`66`, `72`, `73` | USERGUIDE.md, one `## ` section each |
| `61-running-quillo` | USERGUIDE.md's opening: commands, options, settings, desktop apps, editors |
| `12`, `23`, `32`–`36`, `41`, `42`, `51`, `52`, `54`, `55` | The website's guides (quillo-website/guides/*.md) |

The order is the documentation site's (docs.qwe), in six parts. Headings are one level
up from the sources (a chapter is `#`), and every link between pages points inside the
book: to a heading's ID, given as `{#id}` where two headings would otherwise share one.

`assemble.py` made the chapters it can make from their sources, and checks that every
`#` link in all of them, the hand-written ones too, lands on a heading. Running it again
**overwrites those chapters**, so edits made to them here are lost; it is for the next
edition, against a newer USERGUIDE.md and guides.

## With each release

The book changes with every release, beside the guide and the changelog: change the
sources, run `assemble.py`, and read what moved.

The version on the copyright page, in the preface and in `metadata.yaml` changes when the
release is cut.

## For AI coding assistants

`llms.md` is the language as an assistant needs it: the syntax on one screen, what
Quillo does not have and what to write instead, routes, components, the database, and a
list to go through before finishing. It is written by hand, and each code block in it
passes `quillo check` and runs. `llms.py` writes the website's `llms.txt` (`llms.md`
and every builtin, from `src/builtin_docs.rs`) and `llms-full.txt` (that and the
chapters, but the copyright page, the preface, the part pages and the author's page),
served at quillolang.org/llms.txt and /llms-full.txt. Run it after `assemble.py`, and
whenever `llms.md` or a builtin changes.

## Left to do

- Topics told twice, as on the site: a guide chapter, and the same subject in the
  reference (route groups, prefetching, cookies and sessions, database, money, services,
  UDP and channels). Keep both, or cut the reference down to what the guide leaves out.
- The README's Performance numbers date fastest; check them before printing.
- An index, and the ISBN on the copyright page.
- The print build: pandoc and typst (`brew install pandoc typst`), with
  `.part` headings as part pages, `.unnumbered` ones without a number, and links
  printed as page references.
