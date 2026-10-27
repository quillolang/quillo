-- The ledger's tables, for PostgreSQL. (sqlite/001_ledger.sql is the same for SQLite.)

-- An account holds one currency. A customer's may not go below zero; the
-- bank's own (cash, the opening balances) may.
create table accounts (
    id text primary key,
    name text not null,
    currency char(3) not null,
    kind text not null check (kind in ('customer', 'bank')),
    unique (id, currency)
);

-- A journal entry: who posted what, and the business day it counts from.
create table entries (
    id text primary key,
    memo text not null,
    value_date date not null,
    created_by text not null,
    created_at timestamptz not null,
    posted integer not null default 0
);

-- Its postings. Amounts are whole minor units (cents), so sums are exact in
-- SQL too. A posting's currency is its account's (the foreign key).
create table postings (
    entry_id text not null references entries (id),
    line integer not null,
    account_id text not null,
    currency char(3) not null,
    amount_minor bigint not null,
    primary key (entry_id, line),
    foreign key (account_id, currency) references accounts (id, currency)
);
create index postings_account on postings (account_id);

-- An entry is made unposted, gets its postings, then is posted: then its
-- postings must add up to zero in each currency, and nothing changes after.
create function ledger_entry_check() returns trigger language plpgsql as $$
begin
    if tg_op = 'INSERT' then
        if new.posted <> 0 then
            raise exception 'an entry is posted after its postings are in';
        end if;
        return new;
    end if;
    if old.posted = 1 then
        raise exception 'a posted entry does not change';
    end if;
    if tg_op = 'UPDATE' and new.posted = 1 then
        if not exists (select 1 from postings where entry_id = new.id) then
            raise exception 'an entry needs postings';
        end if;
        if exists (select 1 from postings where entry_id = new.id group by currency having sum(amount_minor) <> 0) then
            raise exception 'the postings of an entry must add up to zero in each currency';
        end if;
    end if;
    return coalesce(new, old);
end $$;

create trigger entries_check before insert or update or delete on entries
    for each row execute function ledger_entry_check();

create function ledger_posting_check() returns trigger language plpgsql as $$
begin
    if tg_op = 'UPDATE' then
        raise exception 'postings do not change';
    end if;
    if exists (select 1 from entries where id = coalesce(new.entry_id, old.entry_id) and posted = 1) then
        raise exception 'a posted entry does not change';
    end if;
    return coalesce(new, old);
end $$;

create trigger postings_check before insert or update or delete on postings
    for each row execute function ledger_posting_check();
