-- What the bank tells its customers: messages in the app, and email copies
-- sent later (here written down, as there is no mail server).

create table messages (
    id integer primary key,
    customer_id integer not null,
    kind text not null,                        -- payment, account, security, desk
    title text not null,
    body text not null default '',
    created integer not null,
    read integer not null default 0
);
create index messages_by_customer on messages (customer_id, read, id);

create table emails (
    id integer primary key,
    customer_id integer not null,
    subject text not null,
    sent integer not null
);
