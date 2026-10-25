-- Images a workspace uploads, for pages, the shop and its logo. The file
-- is kept as uploaded; the sizes and crops pages use are made from it.

create table media (
    id integer primary key,
    key text not null unique,          -- in addresses: not guessable from the id
    name text not null,
    alt text not null default '',      -- what it shows, for screen readers
    format text not null,              -- jpeg, png, webp, gif
    width integer not null,
    height integer not null,
    size integer not null,
    alpha integer not null default 0,
    uploaded_by integer references users(id) on delete set null,
    created_at integer not null
);
