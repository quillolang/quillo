# How pages are found

A request for `/guide/pages` looks up `/guide/pages` in `NAV` and reads its file,
`pages/guide/pages.md`. A path that is not in `NAV` gets the site's own *Not found* page,
with a 404.

Every page also has its Markdown at the same address with `.md` on the end: this one is
at [/guide/pages.md](/guide/pages.md), sent as `text/markdown`.

`/sitemap.xml` lists every page for search engines, written as an XML literal in
`main.qwe`.
