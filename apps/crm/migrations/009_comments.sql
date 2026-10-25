-- Readers' comments on posts; each waits for a moderator.

create table comments (
    id integer primary key,
    post_id integer not null references posts(id) on delete cascade,
    contact_id integer references contacts(id) on delete set null,
    name text not null,
    email text not null,
    body text not null,
    status text not null default 'pending',     -- pending, approved, spam
    created_at integer not null
);
create index comments_post on comments(post_id, status, created_at);
