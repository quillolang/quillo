-- One workspace (tenant): its people, its sales, its marketing, its flows.

create table settings (
    key text primary key,
    value text not null
);

create table users (
    id integer primary key,
    email text not null unique,
    name text not null,
    password_hash text not null,
    role text not null default 'member',          -- owner, admin, member
    created_at integer not null
);

-- One-time sign-in links (after signing up on the main site).
create table sign_in_tokens (
    token text primary key,
    user_id integer not null references users(id),
    expires_at integer not null
);

create table companies (
    id integer primary key,
    name text not null,
    domain text not null default '',
    industry text not null default '',
    fields text not null default '{}',
    created_at integer not null,
    updated_at integer not null
);

create table contacts (
    id integer primary key,
    first_name text not null default '',
    last_name text not null default '',
    email text not null default '',
    phone text not null default '',
    company_id integer references companies(id) on delete set null,
    owner_id integer references users(id) on delete set null,
    stage text not null default 'lead',            -- lead, prospect, customer, subscriber, churned
    source text not null default '',
    subscribed integer not null default 0,
    tags text not null default '[]',
    fields text not null default '{}',
    created_at integer not null,
    updated_at integer not null
);
create index contacts_email on contacts(email);

create table pipelines (
    id integer primary key,
    name text not null,
    position integer not null default 0
);

create table stages (
    id integer primary key,
    pipeline_id integer not null references pipelines(id) on delete cascade,
    name text not null,
    position integer not null,
    probability integer not null default 0,
    kind text not null default 'open'               -- open, won, lost
);

create table deals (
    id integer primary key,
    title text not null,
    value_cents integer not null default 0,
    stage_id integer not null references stages(id),
    contact_id integer references contacts(id) on delete set null,
    company_id integer references companies(id) on delete set null,
    owner_id integer references users(id) on delete set null,
    close_date text not null default '',
    fields text not null default '{}',
    position integer not null default 0,
    created_at integer not null,
    updated_at integer not null
);

create table tasks (
    id integer primary key,
    title text not null,
    due text not null default '',                   -- YYYY-MM-DD
    done integer not null default 0,
    contact_id integer references contacts(id) on delete cascade,
    deal_id integer references deals(id) on delete cascade,
    owner_id integer references users(id) on delete set null,
    created_at integer not null
);

create table notes (
    id integer primary key,
    body text not null,
    contact_id integer references contacts(id) on delete cascade,
    deal_id integer references deals(id) on delete cascade,
    author_id integer references users(id) on delete set null,
    created_at integer not null
);

-- The timeline: what happened to whom.
create table activities (
    id integer primary key,
    kind text not null,
    contact_id integer references contacts(id) on delete cascade,
    deal_id integer references deals(id) on delete cascade,
    summary text not null,
    created_at integer not null
);
create index activities_contact on activities(contact_id, created_at);

create table custom_fields (
    id integer primary key,
    entity text not null,                           -- contact, company, deal
    key text not null,
    label text not null,
    type text not null default 'text',              -- text, number, date, select, checkbox
    required integer not null default 0,
    options text not null default '[]',
    position integer not null default 0,
    unique (entity, key)
);

create table products (
    id integer primary key,
    name text not null,
    sku text not null unique,
    price_cents integer not null,
    kind text not null default 'one_time',          -- one_time, subscription
    interval text not null default '',              -- month, year (subscriptions)
    description text not null default '',
    stock integer,                                  -- null: not counted
    active integer not null default 1,
    fields text not null default '{}',
    created_at integer not null
);

create table orders (
    id integer primary key,
    number text not null unique,
    contact_id integer references contacts(id) on delete set null,
    status text not null default 'pending',         -- pending, paid, refunded
    subtotal_cents integer not null,
    tax_cents integer not null default 0,
    total_cents integer not null,
    items text not null,                            -- [{ product_id, name, sku, price_cents, qty }]
    payment_ref text not null default '',
    fields text not null default '{}',
    created_at integer not null,
    paid_at integer
);

create table forms (
    id integer primary key,
    slug text not null unique,
    name text not null,
    headline text not null default '',
    fields text not null default '["email"]',       -- which inputs it shows
    tags text not null default '[]',                -- tags given to who submits
    created_at integer not null
);

create table form_submissions (
    id integer primary key,
    form_id integer not null references forms(id) on delete cascade,
    contact_id integer references contacts(id) on delete set null,
    data text not null,
    created_at integer not null
);

create table campaigns (
    id integer primary key,
    name text not null,
    subject text not null,
    body text not null default '',                  -- Markdown
    audience text not null default '{"tag": ""}',   -- who gets it
    status text not null default 'draft',           -- draft, sent
    sent_count integer not null default 0,
    created_at integer not null,
    sent_at integer
);

-- Mail waiting to go, and gone: what campaigns and flows send.
create table outbox (
    id integer primary key,
    to_email text not null,
    subject text not null,
    body text not null,                             -- HTML
    kind text not null,                             -- campaign, flow, receipt, system
    contact_id integer references contacts(id) on delete set null,
    campaign_id integer references campaigns(id) on delete set null,
    status text not null default 'queued',          -- queued, sent, failed
    created_at integer not null,
    sent_at integer
);

create table flows (
    id integer primary key,
    name text not null,
    graph text not null,                            -- { nodes: [...], edges: [...] }
    active integer not null default 0,
    created_at integer not null,
    updated_at integer not null
);

create table flow_runs (
    id integer primary key,
    flow_id integer not null references flows(id) on delete cascade,
    status text not null default 'running',         -- running, waiting, done, failed
    node text not null,                             -- the node it is at
    context text not null,                          -- { contact_id, order_id, ... }
    waiting_for text not null default '',           -- time, purchase
    next_at integer,
    log text not null default '[]',
    created_at integer not null,
    updated_at integer not null
);
create index flow_runs_due on flow_runs(status, next_at);
