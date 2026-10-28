-- Customers: who they are (checked at the desk), and how they sign in.

create table customers (
    id integer primary key,
    number text not null unique,               -- 8 digits, what they sign in with
    first_name text not null,
    last_name text not null,
    birth_date text not null,                  -- YYYY-MM-DD
    email text not null default '',
    phone text not null default '',
    street text not null default '',
    city text not null default '',
    postcode text not null default '',
    country text not null default '',
    pin_hash text,
    failed_pins integer not null default 0,
    locked_until integer,
    created integer not null,
    updated integer not null
);
create index customers_by_name on customers (last_name, first_name);
