-- The ledger's tables, for SQLite. (postgres/001_ledger.sql is the same for PostgreSQL.)

-- An account holds one currency. A customer's may not go below zero; the
-- bank's own (cash, the opening balances) may.
create table accounts (
    id text primary key,
    name text not null,
    currency text not null,
    kind text not null check (kind in ('customer', 'bank')),
    unique (id, currency)
);

-- A journal entry: who posted what, and the business day it counts from.
create table entries (
    id text primary key,
    memo text not null,
    value_date text not null,
    created_by text not null,
    created_at text not null,
    posted integer not null default 0
);

-- Its postings. Amounts are whole minor units (cents), so sums are exact in
-- SQL too. A posting's currency is its account's (the foreign key).
create table postings (
    entry_id text not null references entries (id),
    line integer not null,
    account_id text not null,
    currency text not null,
    amount_minor integer not null,
    primary key (entry_id, line),
    foreign key (account_id, currency) references accounts (id, currency)
);
create index postings_account on postings (account_id);

-- An entry is made unposted, gets its postings, then is posted: then its
-- postings must add up to zero in each currency, and nothing changes after.
create trigger entries_made_unposted before insert on entries when new.posted != 0
begin
    select raise(abort, 'an entry is posted after its postings are in');
end;

create trigger entries_balance before update of posted on entries when new.posted = 1 and old.posted = 0
begin
    select raise(abort, 'an entry needs postings')
    where not exists (select 1 from postings where entry_id = new.id);
    select raise(abort, 'the postings of an entry must add up to zero in each currency')
    where exists (select 1 from postings where entry_id = new.id group by currency having sum(amount_minor) != 0);
end;

create trigger entries_stay_posted before update on entries when old.posted = 1
begin
    select raise(abort, 'a posted entry does not change');
end;

create trigger entries_not_deleted before delete on entries when old.posted = 1
begin
    select raise(abort, 'a posted entry does not change');
end;

create trigger postings_frozen before insert on postings
when (select posted from entries where id = new.entry_id) = 1
begin
    select raise(abort, 'a posted entry does not change');
end;

create trigger postings_not_updated before update on postings
begin
    select raise(abort, 'postings do not change');
end;

create trigger postings_not_deleted before delete on postings
when (select posted from entries where id = old.entry_id) = 1
begin
    select raise(abort, 'a posted entry does not change');
end;
