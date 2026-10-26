# Prefetching

Every page Quillo serves asks for the pages it links to before they are clicked. Point
at a link, and the page it leads to is fetched; by the time the click lands, the page is
already there. There is nothing to write or set up: it is on for every page.

> **Try it here.** This site is a Quillo program, and its pages prefetch. Open DevTools
> on the **Network** panel (filter: All) and clear it, then point at a page in the
> sidebar for a moment. A request for that page appears before you click: of type
> **prefetch** in Chrome and Edge, **fetch** in Firefox and Safari. Click, and the page
> shows at once. Chrome also lists each one under **Application → Speculative loads**.

### When a link is prefetched

- **Pointed at for 65 ms**, or at once when it is pressed, touched or reached with Tab.
  Each link is asked for once per page view.
- **Chrome and Edge** are given the link as a speculation rule, and keep the page for the
  navigation themselves.
- **Other browsers** fetch it in the background with `x-quillo-prefetch: 1`. The server
  answers with `cache-control: private, max-age=30`, so the click is answered from the
  browser's cache for the next 30 seconds.

These links are left alone:

- links to other sites, and links with `target` or `download`;
- links to the page itself (`#section`);
- links inside an element with `data-no-prefetch`;
- links a page's components follow themselves (see
  [Single-page apps](/docs/components/#single-page-apps));
- every link, on a connection that is saving data or on 2G.

### Prefetching changes nothing

A route asked for ahead of a click may read (the database, files, other servers) but not
write. Writing to the database, assigning a global variable, setting cookies or the
session, writing files, `publish`, `http.post` and the like, and calls to services stop
it, and the server answers "not prefetched" (`204`, `cache-control: no-store`). The page is
then made as usual when the link is followed.

```quillo
route GET /count {
    db.exec("insert into visits values (1)")     // a write: a prefetch stops here
    <html><body>counted</body></html>
}

route GET /logout {
    end_session()                                // a cookie: not prefetched either
    redirect("/")
}
```

Pointing at a link to either does nothing: `/count` counts only when it is followed, and
pointing at "Log out" logs no one out. The same holds for prefetches the browser makes on
its own (`Sec-Purpose: prefetch`, from `<link rel="prefetch">` or speculation rules a page
writes itself).

### Turning it off

- `data-no-prefetch` on an element turns it off for the links inside it:
  `<nav data-no-prefetch>…</nav>`.
- A route that sets its own `cache-control` keeps it: a prefetch of it is answered as the
  route says.
- `quillo run --no-prefetch` turns it off for the whole program: pages no longer carry
  it, and prefetch requests are answered as ordinary ones.

### When you see no requests

- **Point at a page other than this one.** Links to the same page and to other sites are
  never prefetched, and each link only once per page view: reload to try a link again.
- **Look at the right filter.** Choose All in the Network panel: Chrome lists these with
  the type "prefetch", which narrower filters may leave out.
- **Chrome's own settings, and its extensions.** Chrome ignores speculation rules, and
  nothing is fetched, when **Preload pages** is off (Settings → Performance), when Energy
  Saver is on, or when an extension turns prediction off: uBlock Origin's **Disable
  pre-fetching** (Settings → Privacy) does, and it is on by default. A page that
  prefetches in Edge but not in Chrome is usually this. **Application → Speculative
  loads** says when Chrome refused one, and why.
- **Check the page's part.** After pointing at a link, in the console,
  `document.querySelectorAll('script[type=speculationrules]').length` is 1 or more in
  Chrome: the page has asked, and anything missing is the browser's choice.

### Behind a CDN or on a static host

Quillo marks only prefetched pages as keepable for 30 seconds, by the request header.
A cache or a static file server in front may not tell the two kinds of request apart,
and then the browsers that fetch in the background re-ask on the click. Chrome and
Edge do not depend on it.

This site is served as static files by LiteSpeed, which keeps a file's headers whatever
the request, so every page is sent with `cache-control: private, max-age=30`. A page
then may be up to 30 seconds old after a change, which suits documentation.
