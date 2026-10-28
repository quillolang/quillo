-- Transfers as they were asked for and decided: done at once, held for a
-- supervisor (then approved or rejected), refused, or failed.

create table transfers (
    id integer primary key,
    customer_id integer not null,
    from_id integer not null,
    to_id integer not null,
    to_number text not null,
    cents integer not null,
    currency text not null,
    memo text not null default '',
    status text not null,                      -- done, held, denied, rejected, failed
    reasons text not null default '',          -- why risk held or refused it, one a line
    journal_id integer,
    decided_by text,
    decided_at integer,
    created integer not null
);
create index transfers_by_status on transfers (status, id);
create index transfers_by_customer on transfers (customer_id, id);
