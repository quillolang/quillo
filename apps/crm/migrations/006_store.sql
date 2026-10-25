-- A fuller shop: products with pictures and a category, a cart, discount
-- codes and shipping.

alter table products add column images text not null default '[]';     -- media values, the first is the main one
alter table products add column category text not null default '';
alter table products add column compare_at_cents integer;              -- the price before, shown struck through
alter table products add column featured integer not null default 0;
alter table products add column digital integer not null default 0;    -- nothing to ship

alter table orders add column discount_code text not null default '';
alter table orders add column discount_cents integer not null default 0;
alter table orders add column shipping_cents integer not null default 0;

create table discounts (
    id integer primary key,
    code text not null unique,               -- upper case
    kind text not null,                      -- percent, amount, shipping (free shipping)
    value integer not null default 0,        -- percent, or cents
    min_cents integer not null default 0,    -- the subtotal it needs
    max_uses integer,                        -- null: no limit
    uses integer not null default 0,
    ends_at integer,                         -- null: no end
    active integer not null default 1,
    created_at integer not null
);
