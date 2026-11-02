# Preface {#preface .unnumbered}

A web app is usually several languages held together by conventions: a server language
for the routes, a template language for the HTML, JavaScript and a framework for what
runs in the page, SQL for the data, and the JSON, `fetch` calls and API routes that pass
values between them. Most of the work, and most of the bugs, are at the joins.

Quillo removes the joins. Routes, HTML, components and database queries are its syntax,
and one file holds the server and the browser: what the program answers, the pages it
renders, the components that run in the page and the database they use. A component
calls a function on the server as it calls any other function, and the checker reads
the whole program, both sides, before it runs.

I wrote Quillo, and its user guide, as the language grew. The guide is a reference: it
says everything, as briefly as it can, in the order of the language's parts. This book
is made from the same material, set in the order you need it when you learn: from a first
program to a server in production. Where the online documentation has a guide that walks
through a subject with a working program, that guide is a chapter here; where it has a
reference, that reference is here as well, at the back.

## Who this book is for

This book is for programmers who have built something for the web, in JavaScript or
TypeScript, Python, PHP, Ruby, Go or another language. It does not teach programming,
HTML or SQL, and it assumes you know what a route, a request and a database table are.
It assumes no knowledge of Quillo.

Quillo reads like JavaScript, and the book says so where the two differ. It also
compares Quillo with the frameworks you may know (React, Svelte, Vue, Laravel) where that
is the quickest way to say what something does. You do not need to know any of them.
Rust appears in one chapter, for the programs that want it; nothing else needs it.

## Which Quillo

This book describes **Quillo 0.6.0**, released in October 2026. Quillo is young, and the
language may still change before 1.0. The user guide at **quillolang.org/docs** follows
the latest release; when a later version differs from what is written here, the guide
is right. [Appendix B](#not-built-yet) lists what was not built yet when this book was
written.

## How this book is organized

**Part I, Getting started,** says what Quillo is ([Introduction](#introduction)), how to
install it ([Installing](#install)), what it can and cannot do ([Possible or
not](#possible-or-not)), and then goes through the language itself ([Language
tour](#language-tour)).

**Part II, Pages,** is about what runs in the browser: [components](#components), how
they follow the values they show ([Reactivity](#reactivity)), and how a page calls the
server without `fetch` or `async` ([No fetch, no async/await](#async)).

**Part III, Serving,** is about what runs on the server: [routes](#routes), [route
groups](#route-groups), [headers, cookies and sessions](#headers-cookies-sessions), [real
time](#realtime) over UDP, channels and media, [prefetching](#prefetching), and
[validation](#validation) that runs in the browser and on the server alike.

**Part IV, Data,** covers [databases](#database), from one SQLite file to shards and one
transaction over several databases, and [money](#money): exact amounts, payments done
once, and a small ledger.

**Part V, Larger programs,** is about programs that grow: how to [structure](#structure)
one, how to split it into [services](#microservices), how to add [Rust](#rust), and how to
[host](#hosting) it, on a server of your own or on [a shared one](#shared-hosting).

**Part VI, Reference,** has the details the other parts leave out: [every command and
option](#running-quillo), the [standard library](#standard-library), the
[checker](#checking), [concurrency and reloading](#concurrency-and-reloading), [cached
views](#cached-views) and [compiled numbers](#compiled-numbers). Read it when you need
it.

The appendices list the [example programs](#examples), what was [not built
yet](#not-built-yet) for this release, and [how Quillo itself is built](#layout).

Parts I and II are best read in order. After them, read the chapters you need: each one
points to the others where they meet.

## Conventions

Quillo code is set like this, and a comment after a line often shows what it gives:

```quillo
let total = money.mul(money.of(19.99m, "USD"), 3)       // 59.97
```

Commands for a terminal are set the same way, with `#` comments:

```sh
quillo run app.qwe --watch        # serve it, and reload as it changes
```

A `.qwe` file is a Quillo program; a `.qwer` or `.rs` file is Rust that a program uses. Commands
are written as `quillo ...`, for a `quillo` on your `PATH`; if you built it yourself and
did not install it, that is `./target/release/quillo ...` in Quillo's folder.

## The examples

The example programs are in **github.com/quillolang/examples**, and some chapters run
them from a copy beside Quillo's own folder (`../quillo-examples`). [Appendix
A](#examples) lists them.

## Getting help

Questions, help, and what people are building: the Quillo Discord (linked from
quillolang.org) and **r/quillo** on Reddit. Bugs and requests for the language go to
**github.com/quillolang/quillo/issues**, and so do mistakes in this book.
