-- The ledger: accounts, and a journal of entries whose postings add up to
-- zero (double entry). Amounts are whole minor units (cents), so SQLite's
-- sums are exact.

create table accounts (
    id integer primary key,
    number text not null unique,               -- an IBAN, QB + 18
    customer_id integer not null,              -- 0 for the bank's own (the vault)
    kind text not null,                        -- checking, savings or vault
    currency text not null,
    name text not null,
    status text not null default 'open',       -- open, frozen or closed
    balance integer not null default 0,        -- kept with each posting, checked against them
    opened integer not null
);
create index accounts_by_customer on accounts (customer_id);

create table journal (
    id integer primary key,
    kind text not null,                        -- transfer, deposit, withdrawal, opening
    memo text not null default '',
    ref text unique,                           -- the caller's id for it: posting it twice posts it once
    created integer not null
);

create table postings (
    id integer primary key,
    journal_id integer not null references journal(id),
    account_id integer not null references accounts(id),
    amount integer not null,                   -- + in, - out
    balance_after integer not null,
    created integer not null
);
create index postings_by_account on postings (account_id, id);
