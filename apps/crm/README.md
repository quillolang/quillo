# Relay — a CRM in Quillo

A multi-tenant CRM for merchants, bloggers and news outlets: contacts, companies, a
deals pipeline, tasks, a shop with checkout, sign-up forms, campaigns, and flows drawn
in a designer. Each workspace is a tenant with a database of its own; plugins add
fields, rules and flow steps, and the rules run both in the browser (hints as you type)
and on the server (where they decide).

```sh
quillo run apps/crm/main.qwe --port 9000 --watch
```

Open <http://localhost:9000>, create a workspace, and you land in it at
`http://<name>.localhost:9000` (browsers send `*.localhost` to this machine).

## What is in it

| | |
|---|---|
| **People** | Contacts with stages, tags, consent and custom fields; companies; a timeline of everything that happened to each; notes and tasks; CSV import (each row checked by the rules) and export. |
| **Sales** | Pipelines with stages, a board you drag deals across; products (one-time and subscriptions), a public shop at `/shop` with a checkout, orders, refunds. |
| **Marketing** | Public sign-up forms at `/f/<form>`, campaigns written in Markdown to all subscribers or a tag, an outbox, signed unsubscribe links. |
| **Pages** | Landing pages and a home page, made of blocks in a designer: a palette of the blocks the plugins switched on bring, a live preview rendered as visitors will see it, drag to reorder, settings per block; a theme (colour, font, dark, logo) per workspace; drafts, and publishing when the page rules pass. |
| **Flows** | A designer: drag steps, connect their ports, branch on conditions, wait, offer a product and wait for the purchase. Runs show step by step, and can be started by hand for a contact. |
| **Workspaces** | Signing up on the main site; presets per industry (pipeline, products, forms, flows, plugins); a team with owner, admin and member roles; settings. |

Payments run in test mode (`4242 4242 4242 4242` pays, `4000 0000 0000 0002` is
declined), and mail stays in the outbox until a mail webhook is set in Settings: then
each mail is POSTed there as `{ to, subject, html }`, the shape email APIs take.

## How it is put together

```
main.qwe            tenants, routes, the plugin list, the job that runs flows and mail
migrations/         each workspace's tables
lib/util.qwe        dates, money, CSV, names: pure, so both sides use them
lib/rules.qwe       validate(kind, record, ctx): every plugin's rules for a kind
lib/records.qwe     contacts, companies, deals, tasks, notes, the dashboard
lib/commerce.qwe    products, the price quote, checkout, orders
lib/marketing.qwe   forms, campaigns, the outbox
lib/flows.qwe       the flow engine: fire, walk, waits, steps, templates
lib/workspace.qwe   signing up, presets, settings, fields, pipelines, team
lib/pages.qwe       pages: drafts, publishing, the theme, rendering blocks
lib/media.qwe       images: uploads, crops, the sizes pages use
plugins/*.qwe       core (always on) and the optional plugins
plugins/core_blocks.qwe   how the core's page blocks look
ui/*.qwe            the pages: public ones, the app, the flow and page designers
```

### Tenants

`tenant_from(CRM_DOMAIN)` makes `acme.example.com` the tenant `acme`, and
`db.open("data/{tenant}.db")` gives each its own SQLite file, migrated from
`migrations/` when made. A workspace's database is made only when it signs up
(`with_tenant` in `create_workspace`); a request for a made-up subdomain gets a 404
and makes nothing. Sessions are bound to the tenant they were made in.

### Rules: one definition, both sides

A rule is a function of a record and its context, giving the problems it sees:

```quillo
(c, ctx) => when(!blank(c.email) && !is_email(c.email), problem("email", "Not an email address"))
```

`validate(kind, record, ctx)` runs the rules of the core and of the plugins the
workspace has on. Editors call it as you type and show each problem under its field
(warnings, `level: "warning"`, in amber); the server functions that save call it again
with the context built from the database, and refuse the save if there are errors.
Rules only compute — no database, no clock (`ctx.today` is the date) — which is what
lets the browser run them: Quillo compiles the plugin list, rules included, into the
page's JavaScript. Kinds: `contact`, `company`, `deal`, `task`, `product`, `checkout`,
`signup` (public forms), `campaign` and `flow` (the designer's graph).

### Pages

Every block of a page comes from a plugin: the core brings hero, text, image, columns,
testimonial, button, space, product, product grid and sign-up form; Consent brings a
privacy note, Memberships the membership plans, Sponsorships a sponsor banner. A
block's description (its type, label, group and fields) is plugin data the designer
reads; how it looks is the plugin's renderer, a function of its settings run on the
server (for the preview and for visitors); what makes it acceptable is the plugin's
`page` rules, plus the checks every block gets from its fields' types (required, links,
products for sale, forms that exist). A draft saves as it is; publishing refuses while
the rules find errors. Visitors see `/p/<slug>`, or the home page at the workspace's
own address.

### Images

Media keeps a workspace's images: uploaded from the library or from any image setting
(blocks, the logo), checked to be JPEG, PNG, WebP or GIF up to 10 MB, and kept as they
came. A block holds the image with its crop (in the image's own pixels, made in the
crop editor with free or fixed ratios), so the crop can be changed and the image is
never altered. Pages ask for `/media/<key>/<crop>/<width>`, at fixed widths from 160
to 1920 with a `srcset` for the browser to choose from; each size is made on first
request with `image.convert` and kept, sent with a year's cache. Keys are random, so a
workspace's images cannot be listed by counting.

### Flows

A flow is `{ nodes, edges }`: one trigger, and steps joined port to port. When its
event happens (`fire("order_paid", {...})`) a run starts and walks the graph, each step
leaving by a port (`out`, or `yes`/`no`). Steps that wait park the run; the job (every
10 seconds, for each workspace) carries on those whose time has come, and a purchase
carries on those waiting for it. A contact is in a flow once at a time, and a run
stops after 100 steps. A flow can be switched on only when the flow rules find no
errors — the same rules the designer shows.

## Writing a plugin

A plugin is a file in `plugins/` with a map the browser can run (fields, rules, the
steps' descriptions) and, apart from it, the functions its steps run on the server:

```quillo
let plugin_birthday = {
    id: "birthday",
    name: "Birthdays",
    description: "Remembers birthdays; flows can wish happy birthday.",
    audience: ["merchant", "blogger"],
    fields: [{ entity: "contact", key: "birthday", label: "Birthday", type: "date" }],
    rules: {
        contact: [(c, ctx) => {
            let b = (c.fields ?? {}).birthday
            when(!blank(b) && !blank(ctx.today) && b > ctx.today, problem("fields.birthday", "A birthday in the future?"))
        }],
    },
    steps: [{ type: "birthday.wish", label: "Wish happy birthday", group: "Message", outputs: ["out"], fields: [] }],
}

fn birthday_wish(config, run) {
    let c = run_contact(run)
    queue_mail(c.email, "Happy birthday!", mail_html("Many happy returns, {{first_name}}!", c), "flow", c.id, null)
    { port: "out", log: "wished happy birthday" }
}

let runners_birthday = { "birthday.wish": birthday_wish }
```

A plugin can bring page blocks the same way: their descriptions under `blocks` in the
plugin map (`{ type: "birthday.card", label: "Birthday card", group: "Grow", fields:
[...] }`, with field types `text`, `textarea`, `url`, `link`, `select`, `product`,
`form`), their rules under `rules.page`, and their renderers apart, on the server:
`let renderers_birthday = { "birthday.card": (c, ctx) => <section>...</section> }`.

Then in `main.qwe`: `use "plugins/birthday.qwe"`, add `plugin_birthday` to `plugins`,
`runners_birthday` to `step_runners` and any renderers to `block_renderers`. Workspaces switch it on in Plugins. A step's
runner gets its node's settings and the run (`run.context.contact_id`, `order_id`, ...)
and says which port to leave by; returning `{ waiting_for: "time", until: <ms>,
resume: "out" }` parks the run instead. Plugins can also bring `checkout_fields`,
`form_fields` and `taxes: (order, subtotal_cents, ctx) => cents`.

## One workspace, or many

By default the CRM is a service: workspaces sign up on the main site and each gets a
subdomain. To install it for one business on its own domain instead:

```sh
CRM_MODE=single quillo run apps/crm/main.qwe --port 9000
```

The first visit opens a setup page (the business, the owner), and after that the CRM
is that one workspace, at the domain's own address; no subdomains or wildcard DNS. The
two modes are the same program: with one workspace, every request belongs to it.

## Running it for real

- Many workspaces: `CRM_DOMAIN=example.com` and a wildcard DNS record (`*.example.com`); one: `CRM_MODE=single`. Either way behind a proxy
  that terminates HTTPS (limer, Caddy, nginx); set `CRM_SCHEME=https`.
- `QUILLO_SECRET` to a long random string, the same on every server: it signs sessions.
- `data/` holds every workspace's database: back it up.
- Set a mail webhook in each workspace's Settings to send mail for real.
