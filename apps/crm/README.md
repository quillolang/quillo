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
| **Sales** | Pipelines with stages, a board you drag deals across; products (one-time and subscriptions) with pictures, categories, sale prices and variants (sizes, colours: each with its stock and price); a shop at `/shop` (by category, searched, sorted), a page per product, a cart, discount codes, shipping, a checkout; orders, refunds that restock. |
| **Marketing** | Public sign-up forms at `/f/<form>`, campaigns written in Markdown to all subscribers or a tag, an outbox, signed unsubscribe links. |
| **Pages** | Landing pages and a home page, made of blocks in a designer that works as Gutenberg does: the page, rendered as visitors will see it, is edited where it shows — text typed into in place (Markdown kept) and formatted from a toolbar over it — bold, italic,
code, links (⌘B, ⌘I, ⌘K; a link's address is checked), headings, quotes and lists, "## ", "- ",
"1. " or "> " typed at the start of a line turning it, Enter on an empty last list item leaving
the list for a paragraph — + between blocks or `/` to add one (Enter in a text is its next paragraph; Backspace in an empty one removes it), a toolbar on the chosen block (move, add before or after, copy, remove), ⌘Z and ⌘⇧Z;
Columns (2 to 4) hold blocks of their own — an empty column suggests Text, Image, Testimonial,
Button or any block, and blocks are dragged into a column, from the library or by their handle
(a columns block stays in the page, not in a column; columns from before, Markdown left and
right, become a text block in each); the rules, the outline and the sections a page uses see
the blocks in columns too; a live preview, at a secret link (/preview/<token>, no sign-in: another
browser, a phone, a client), shows the page as the designer has it on screen, saved or not, as
visitors would see it, following each change as it is typed (`live`) and the block being edited
(a new link stops the old one; not for search engines, the link not passed on). Whoever has the link — a customer — gives feedback there: Give feedback, a click on the block it is about, Snapshot and comment; they draw on the block (three colours, undo, clear) and say what they think, under their name (kept in their browser). The snapshot is the block as the server renders it then, and the strokes in its own pixels, so it shows where it was drawn at any size (zoomed to the marks). The team sees each as a to-do in the designer's Feedback tab (a count on it and on the block), acknowledges it, answers; both sides see the talk as it goes (`live`), and the viewer marks it resolved, or opens it again. Those who opened the page lately (else the owners) are told by mail; the blocks' library or the outline on the left, the page's or the block's settings on the right. A block's renderer marks what can be typed into in the designer's preview only — `data-edit="field"`, and how it is formatted: `data-md="1"` Markdown (paragraphs, headings, lists, quotes), `data-md="inline"` a line of it (bold, italic, code, links: headlines, quotes, captions, headings; rendered with `markdown(text, { inline: true })`), `data-md="lines"` a list, one item a line, none plain text (labels, names). The core blocks and the plugins' (blog, sponsors, membership plans) are marked; other fields are set in the sidebar. Saving sends the progress, not the page (`save_changes`: the blocks that changed, the order, the settings that changed; the others kept as saved, so what someone else saved meanwhile stays), and the designer stays as it is — the block chosen, the caret, the undo history; the button says how many changes wait, ⌘S saves them. Sections made once and put on many pages; drafts, and publishing when the page rules pass. |
| **Theme** | Themes from plugins (Clean, Editorial, Boutique, Journal from the core), each with its header and width; colour, font, dark, logo; the menu and the footer; CSS of your own in a highlighting editor; the site shown as it will look while you change it. Every public page (pages, blog, shop, forms) is in it, with link previews, a sitemap and robots.txt. |
| **Blog** | Posts with covers, categories, tags and members-only posts at `/blog`; search, author pages, reading time, related posts, sharing, readers' comments (checked by the comment rules, approved by a moderator); code in posts highlighted; an RSS feed; news blocks for any page: top stories, a category's row, most read, breaking news (the Blog plugin). |
| **Flows** | A designer: drag steps, connect their ports, branch on conditions, wait, offer a product and wait for the purchase. Runs show step by step, and can be started by hand for a contact. |
| **Objects** | Kinds of records of your own (vehicles, events, tickets…), designed in the app: fields of every type, links to contacts, companies and other objects' records, lookups (a field of what a link leads to, kept up to date), rules made without code (both sides), rule scripts on fields (on the server), templates to start from; each object's records in the menu, with a list, a page per record, CSV export, and flows that start when one is added or changed. |
| **Desks** | Each category of posts can have its own editors, reviewers and publishers, and the approvals a post needs: draft → review → approved → published, with changes asked for by note, mail at each step, and a queue of what waits for you. |
| **Questions and notes** | The app asks before it deletes, and tells you what it did, in dialogs and notes on the page (`ask`, `notify`), in a notice style plugins bring (Standard, Quiet; Glass and Bold from the Notice styles plugin): where they open and how they look. |
| **Newsroom suite** | A plugin whose model is a newspaper's: Categories (which can depend on one another: Elections, part of Politics) and Articles, with their states (Draft, In review, Changes asked, Approved, Published) and moves for the Writers, Reviewers and Publishers groups, all the workspace's own to design further. Reviewers mark any words or picture and comment; threads are answered and resolved; moves tell the next people. Articles are the site: a front page, a page per section, a page per story. |
| **Roles and groups** | Roles: what each may do with each object (see, edit, delete), each field (edit, read, hidden), and an access script deciding which records they see. Groups: people who work together, who get the group's roles and are what it is on the team (designers, writers, reviewers, publishers). |
| **States** | Objects have states and the moves between them: each move for some groups or roles, with a form of the fields it needs; the state changes by a move and nothing else. Flows start when a record moves, ask a group or a role to decide (approved / rejected), and move records. |
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
lib/site.qwe        the frame of public pages: themes, menu, footer, sitemap
lib/pages.qwe       pages and sections: drafts, publishing, rendering blocks
lib/media.qwe       images: uploads, crops, the sizes pages use
lib/blog.qwe        the blog: posts, /blog, readers, the feed
plugins/*.qwe       core (always on) and the optional plugins
plugins/core_blocks.qwe   how the core's page blocks look
plugins/core_themes.qwe   the core's themes
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

### The blog

The Blog plugin (on for bloggers and news outlets) makes a workspace's site a blog:
posts written in the app (Posts) in Markdown, with a cover from Media, tags, an excerpt
and a description for search results; `/blog` lists them (by tag, a page at a time),
`/blog/<post>` shows one, and `/blog/feed.xml` is the RSS feed. Its post rules hint in
the editor and decide when a post is published. With Memberships, a post can be for
members: others see its excerpt and how to become one, and readers sign in with a link
sent to their email (the same answer for any address, so the form does not tell who is
a reader). A published post can become a campaign to subscribers. Its "Latest posts"
block puts the newest posts on any page, such as the home page.

### Themes

A theme is plugin data, like a block: `{ id, name, description, header, width, color,
font, css }`. `header` lays out the top of every page (`bar`, `center`, `masthead` with
the date and a menu bar, `split` with the cart on the right); `css` is scoped to
`.theme-<id>`. A workspace picks a theme and overrides its colour and font, sets its
logo, menu and footer, and may add CSS of its own, which is nested in `.site` so it
styles the site and nothing else (and cannot contain `<`). Switching off the plugin a
theme came from goes back to Clean; the Plugins page says so first, as it does for the
pages and flows a plugin's blocks and steps are in.

### The shop

Products have pictures (from Media), a category, a price before (shown struck through)
and stock; digital products and subscriptions are not shipped. The cart is in the
visitor's session and its forms work without JavaScript. `quote_lines` computes a cart's
subtotal, discount (a percentage, an amount, or free shipping, from a code with a minimum
and a number of uses), shipping (one rate, free over an amount) and tax (from plugins) —
the cart and the checkout show it, and the server charges it. Paying takes the stock,
counts the code's use, and starts the purchase flows for each product. A product in
variants asks which on its page; the cart keeps each variant apart (`sku~variant`), and
paying and refunding count each one's stock.

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

### Objects, roles and scripts

Each object's records are a table of their own, with a column per field, `f_<field>`,
typed as the field is: numbers, yes/no as 0 or 1, links as references to the contacts,
companies or records they lead to (indexed; read back as their ids). The table is named
when the object is made (`o_` and letters, digits and `_`: "Its table", after the plural
until it is typed; objects from before are `o_<key>`), and the designer shows, copies
and renames it. A rename moves the table only: the object keeps its key, so its address,
roles, flows, comments and the links to it (the database rewrites those) stay as they
are; SQL of your own that names the table needs the new name. Designing the object changes
its table — a field added is a column added, a field removed a column dropped, a unique
field a unique index the database keeps — and a field's type stays as it was made. Table
and column names go into SQL with `sql_name`, which takes only letters, digits and `_`.
Records from before tables (one shared table, values as JSON) are copied in once, ids
kept; the old rows stay as a backup.

Many objects and pages: the Objects and Pages pages find, sort (name, records, changed
or opened lately) and group by folder, each group folding; objects show as cards or a
table (a table past twelve). Pinned objects and pages (a star, for everyone) come first,
and the sidebar's Workspace group has them under Objects and Pages, then what each person
opened lately (eight at most, live). The search at the top (⌘K) finds objects, pages
and the records of every object (only those someone may see), with the arrows and Enter;
choosing an object (a link field's, a flow's) is typed to narrow. The workspace no longer
carries every object's fields (each object brings its own), record counts come in one
query over every table, and a member's roles are read once, not once an object; with 100
objects, `load_workspace` went from 208 to 28 KB and the sidebar from 4,288 to 776 px.

Objects with many fields: a record's form lays its fields out in a grid (two or three
across; long text, Markdown and pictures take a row), with Save kept in view; its list
scrolls sideways under its first column, and shows the columns its designer picks
("Columns", the same for everyone) or else the first eight short fields.

An object's fields are custom fields (`obj.<key>`); a record's values are checked by
`validate("obj.<key>", record, ctx)`: its fields' types, the rules made in the designer
(`{ if, then, message, warning }`, data both sides run), and any plugin's rules for that
kind. A field's **rule script** and a role's **access script** are Quillo that the
workspace writes and the server runs with `run_script`: in a sandbox, they see only the
values they are given (`value`, `record`, `user`, `today`, and helpers such as
`others_with(field, value)`) and builtins that compute — no database, files, network or
clock — and stop after 20,000 steps. A rule script gives `true` (or null) when the value
is fine, `false` or a message when not; forms show its answer as you type, and saving is
refused while it has one. An access script gives `true` for the records a role sees.

### Objects on the site, review, and models from plugins

An object can be on the site (in its designer): its records in a chosen state read as
articles at `/<path>` — a front page, `/<path>/c/<section>` (a section and those that
depend on it), `/<path>/<id>-<slug>` — with which field is the headline, summary,
picture, caption, body, section and byline. Its team previews a record before it is
published (`?preview=1`).

Any record can be reviewed: select a few words of any text field (or click a picture)
and comment; comments are threads, answered and resolved, and stay with the record as
it moves. A move tells the people who make the next moves, and the writer.

A plugin can bring a model: `model: { groups, objects }`, made when it is switched on
where the workspace does not have it yet (the Newsroom suite is one). A move for groups
no one is in is anyone's who may edit, so one person writing alone needs no groups.

To run the CRM as one newspaper: `CRM_MODE=single quillo run apps/crm/main.qwe`, set it
up as a news outlet, and the suite is on.

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

## The plugin studio

Developers write plugins in the app (Plugin studio, in the menu), try them in real pages
and flows, and release them to every workspace. Plugin code runs on the server with the
program's full power, so only the installation's developers may write it:

```sh
CRM_DEVELOPERS=ada@example.com,linus@example.com quillo run apps/crm/main.qwe --reload
```

- A new plugin starts from a template (a block, a rule, a renderer) to change into
  yours. Every top-level name starts with the plugin's id (`events_plugin`,
  `fn events_count`, views `EventsCard`), so plugins never clash.
- **Save draft** checks the plugin the way `quillo check` checks the program, with the
  plugin in it, before writing anything: errors show by line, and a draft that does not
  check is never saved. A saved draft is loaded beside the released version.
- **Try drafts in this workspace**: there, drafts take their released versions' place —
  their blocks in the page designer, their steps in the flow designer, their rules in
  every form. Other workspaces keep the released version.
- **Release** makes the draft the plugin's next version for every workspace; they
  switch it on in Plugins, like the built-in ones.
- **Live**: the editor highlights the code as you type, checks it when you pause (the
  problems on their lines), and saves the draft whenever it checks; once the program has
  loaded it, the preview beside the code shows the plugin's blocks with sample settings
  (a field's `example`, or one of its kind).
- The program takes up what is saved by loading itself again: run it with `--watch`
  (development) or `--reload` (production). Studio plugins are files in
  `plugins/studio/`, to keep in version control with the rest.

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
