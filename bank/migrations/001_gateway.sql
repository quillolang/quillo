-- The gateway's own data: the staff, desk sessions (a clerk filling in a
-- form while the client follows on a phone), what the client was asked to
-- confirm, and an audit log. Customers, accounts and payments are the
-- services'.

create table staff (
    id integer primary key,
    username text not null unique collate nocase,
    name text not null,
    role text not null,                        -- clerk or supervisor
    password_hash text not null,
    created integer not null
);

create table desks (
    id integer primary key,
    code text not null unique,                 -- 6 digits the client types on the phone
    kind text not null,                        -- open_account, withdrawal or update
    clerk_id integer not null references staff(id),
    customer_id integer,                       -- an existing customer, or the one made
    client_token text,                         -- the phone that joined
    data text not null default '{}',           -- the form, as JSON
    pin_hash text,                             -- chosen on the phone; the clerk sees only that it is set
    progress text not null default '{}',       -- what submitting has done so far, as JSON
    status text not null default 'open',       -- open, done or cancelled
    created integer not null,
    updated integer not null
);

create table desk_asks (
    id integer primary key,
    desk_id integer not null references desks(id),
    section text not null,                     -- personal, address, account, deposit, withdrawal, terms
    shown text not null,                       -- what the client confirms, as JSON
    status text not null default 'pending',    -- pending, approved, declined or stale
    note text not null default '',
    asked integer not null,
    answered integer
);
create index desk_asks_by_desk on desk_asks (desk_id, id);

create table audit (
    id integer primary key,
    actor text not null,
    action text not null,
    detail text not null default '',
    created integer not null
);
