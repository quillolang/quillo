# Shared hosting (cPanel) {#shared-hosting}

A shared hosting account, such as cPanel's, often on CloudLinux, gives you a folder and
a web server the host runs: Apache or LiteSpeed, sometimes with NGINX in front. It gives
no root and no systemd. A Quillo program can run there behind FastCGI, with its plugins
built beforehand. This chapter covers how, and what such an account changes. For a server
of your own, see [Hosting](#hosting).

## What is different on a shared server

| | On a shared account | What to do |
|---|---|---|
| **127.0.0.1** | shared with every account on the machine: any of them can connect to a port there, past the web server | listen on a Unix socket in a folder of your own, or answer FastCGI: never a TCP port |
| **Processes** | nothing keeps one of yours running; processes started by hand or by cron are often killed | let the web server start the program: `mod_fcgid` starts it, keeps it up while requests come, and stops it when idle |
| **Rust** | not installed | build the `.qwer` files elsewhere, upload them built, and start it with `quillo fcgi main.qwe --no-build` (an option, as for `quillo run`: see [the options](#running-quillo)) |
| **Limits** | CloudLinux's LVE: processes (`NPROC`, which counts threads, for the whole account) and requests at once (`EP`; over it, visitors get a 508 page from the host) | `quillo fcgi main.qwe --workers 8` or so |
| **System** | AlmaLinux, CloudLinux or CentOS 7 and 8, with old glibc | the Linux releases from 0.7.0 on, linked against glibc 2.17 |

## The binary

The Linux releases from 0.7.0 on are linked against glibc 2.17. They start on
CentOS 7, CloudLinux 7 and 8, AlmaLinux 8 and every newer system. Releases up to 0.6.0
need glibc 2.35. They are glibc builds, not static ones, because a plugin is a library
the binary loads, and a static binary cannot load one.

Put `quillo` in a folder of the account's own, `/home/u/bin/quillo`, and check it starts:

```sh
/home/u/bin/quillo --version
```

## FastCGI with mod_fcgid

`mod_fcgid` is in cPanel's EasyApache 4, and many shared hosts have it on.
`quillo fcgi main.qwe` answers FastCGI on the socket the web server gives it on file
descriptor 0. Apache starts it on the first request, keeps it while requests come, and
stops it when idle.

Keep the program outside the public folder, in `/home/u/app`: its `.qwe` files, its
`.env` and its database. In `public_html` (or the domain's folder) go two files. The
first is `app.fcgi`, made executable with `chmod 755 app.fcgi`:

```sh
#!/bin/sh
cd /home/u/app && exec /home/u/bin/quillo fcgi main.qwe --no-build --workers 8
```

The second is `.htaccess`, which sends every request that is not a file there to it:

```
Options +ExecCGI
AddHandler fcgid-script .fcgi
RewriteEngine On
RewriteCond %{REQUEST_FILENAME} !-f
RewriteRule ^ app.fcgi [QSA,L]
```

- **Each request is the visitor's**: its path, headers and body. `request.ip` is Apache's
  `REMOTE_ADDR`, with no `--trust-proxy` needed (the option of
  `quillo run` that believes a proxy's `X-Forwarded-For`). The server does the rest as it does
  for any visitor: routes, sessions, limits and the log. The log goes to Apache's error
  log.
- **Jobs and the program's own calls** reach it through a socket only it can open, in a
  private folder under the system's temporary one.
- **WebSockets do not pass through FastCGI.** Live data falls back to plain requests, each
  held until a change or 25 s, so live pages still follow changes. Channels over a
  WebSocket do not work, and WebTransport needs a UDP port of its own.
- **A `stream` or `events` answer arrives whole** unless the host sets `mod_fcgid`'s output
  buffer to 0 (`FcgidOutputBufferSize 0`).
- **Several processes.** `mod_fcgid` may start more than one, and each has its own global
  variables, as with `quillo run main.qwe --instances 4`. Keep what must be shared in the database.
  `quillo fcgi` refuses `--split`, `--instances`, `--register` and `--join`.
- A request Apache abandons while its answer streams ends when the connection closes.

Run from a terminal, `quillo fcgi` says what it needs:

```
quillo: file descriptor 0 is not a socket: `quillo fcgi` is started by a FastCGI process manager (mod_fcgid), or given --listen unix:PATH or --port
```

On a server of your own, `quillo fcgi main.qwe --listen unix:f.sock` makes its own socket.
It is short and relative to the folder `quillo` runs in, as a socket's path is at most
100 bytes. Then nginx passes requests to it:

```
location / {
    include fastcgi_params;
    fastcgi_pass unix:/srv/app/f.sock;
}
```

## A Unix socket behind mod_proxy

Where the host sets it up (a VPS with WHM, or a host that offers Quillo), the program can
run as a server on a Unix socket, with Apache proxying to it:

```sh
quillo run main.qwe --listen unix:app.sock --trust-proxy 127.0.0.1
```

```
ProxyPass "/" "unix:/home/u/app/app.sock|http://localhost/" upgrade=websocket
ProxyPassReverse "/" "http://localhost/"
```

`upgrade=websocket` needs Apache 2.4.47 or later. `ProxyPass` belongs in the host's
configuration, not in `.htaccess`.

- **Its mode is 0600** when it is made: only the account's own processes can open it.
  Give `--socket-mode 660` when the web server reaches it through its group:
  `quillo run main.qwe --listen unix:app.sock --socket-mode 660 --trust-proxy 127.0.0.1`.
- **It goes when the server stops.** A socket left by a server that did not stop cleanly
  is taken over. One another server still answers on is refused: `a server is listening
  there`.
- **`--trust-proxy 127.0.0.1`.** Every visitor through the socket is 127.0.0.1 to the
  program, so they share one address and one set of limits. The server says so at start
  until the option is given. With it, `request.ip` is the visitor's, from the
  `X-Forwarded-For` header Apache adds.
- A process manager may pass the socket's path in `$PORT`, as an absolute path
  (`/home/u/app/app.sock`, or `unix:/home/u/app/app.sock`).
- Not with `--split`, `--instances`, `--register` or `--join`.

## Plugins, built beforehand

The account has no Rust, and a build on the server would run each crate's build
scripts there.

1. Build the program's `.qwer` files with `quillo build main.qwe` on Linux, on the same
   processor as the server. Use a system no newer than the server's: a plugin linked
   against a newer glibc does not load on an older one. A CentOS 7 or AlmaLinux 8
   container works.
2. Upload `.quillo/` with the program. It holds `.quillo/native/`, the libraries, and
   `.quillo/plugins.lock`, each library's SHA-256.
3. Run with `--no-build`. Nothing is built on the server. A library that is not the one
   the lock names is refused rather than built again:

```
quillo: will not load the library of ./hi.qwer: libhi-687dd0b24f29e5cf.so is not the library that was built (its SHA-256 differs from .quillo/plugins.lock's); build it with `quillo build`, which writes the lock, and bring .quillo along
```

A plugin called inside a `with` function needs a `//! effects:` line in its header. A
plugin without one may do anything, so it is refused there (see
[Rust](#rust)).

## A policy

The host's `/etc/quillo/policy.toml` says what any program on the server may do. A
`quillo.policy` beside the program narrows it for the site. Each file only takes away:

```toml
# quillo.policy, beside main.qwe
[plugins]
build = false
allow = ["csv@1", "qr"]

[http]
private = false
hosts = ["api.stripe.com"]

[env]
names = ["QUILLO_SECRET", "DATABASE_URL"]

[limits]
workers = 8
```

A limit takes what its option takes: `workers = 0` is refused (`` p.toml:2: `workers` is
a positive number, as `--workers` takes ``), and so is a `max_body` or `max_compute` of
0. The start line lists the limits that hold the program: `quillo: policy
quillo.policy (workers 8)`. A request the policy's `max_compute` ends is a 500, as one
`--max-compute` ends, and the log names whichever is the tighter (`this request
computed for longer than the policy p.toml allows (max_compute)`); a page is told `the
server's policy`, with no path of the server's.

`quillo check --policy policy.toml main.qwe`, run before uploading, says what the
program would be refused. The sections and what each does are in [The
policy](#the-policy).

A policy binds the `quillo` that reads it. On a shared server, a user who can upload a
binary of their own can run it without one: the boundary between accounts is the
system's (Unix users, CloudLinux's CageFS and LVE limits).

## Threads and ulimit -u

A Quillo server may use `--workers` + 9 threads: 73 with the default of 64. On Linux,
the account's process limit counts threads, beside PHP, cron and SSH. Give
`--workers 8` or so; a host's policy can cap it with `[limits] workers`.

At start, a server says when `ulimit -u` is below what it may use:

```
quillo: the process limit (ulimit -u) is 30, and this server may use 73 threads: requests may wait for a thread; raise it, or give --workers fewer
```

A thread the system refuses later is said once, and the requests wait for the threads
there are.

## The rest of the account

- **Settings.** Use the `.env` beside the program, or `--env-file` for one elsewhere
  (`quillo fcgi main.qwe --env-file /home/u/app.env` in `app.fcgi`). Keep secrets out of `app.fcgi`.
- **Jobs.** A FastCGI process stopped when idle runs no `schedule` or `every`. Run them
  from cPanel's Cron Jobs page instead, as a script:
  `cd /home/u/app && /home/u/bin/quillo run jobs.qwe`.
- **The database.** Use SQLite in a folder outside `public_html`, or the host's MySQL with
  `db.open(env("DATABASE_URL"))`.
- **Paths.** A link out of the program's folder is refused, so a link to another
  account's files leads nowhere (see
  [Paths and links](#paths-and-links)).
- **Not built yet:** Passenger, which is cPanel's Application Manager and starts Node.js,
  Python and Ruby apps only, and a WHM plugin that installs Quillo for every account.

## Before going live {#shared-hosting-before-going-live}

- `quillo` from a release linked against glibc 2.17, in a folder of the account's own
- the program outside `public_html`; `.htaccess` and `app.fcgi` in it
- `.quillo/native/` and `.quillo/plugins.lock` uploaded, and `--no-build`
- `//! effects:` on the plugins called inside a `with`
- `QUILLO_SECRET` set in the `.env` beside the program
- `--workers 8`, and `quillo check --policy` passing
- scheduled work in cron, not in `schedule`
