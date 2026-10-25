-- Blog posts (the Blog plugin), and the links readers sign in with.

create table posts (
    id integer primary key,
    slug text not null unique,
    title text not null,
    excerpt text not null default '',
    body text not null default '',               -- Markdown
    cover text not null default '',              -- an image setting (JSON), or ''
    tags text not null default '[]',
    description text not null default '',        -- for search results
    members_only integer not null default 0,
    status text not null default 'draft',        -- draft, published
    author_id integer references users(id) on delete set null,
    created_at integer not null,
    updated_at integer not null,
    published_at integer
);
create index posts_published on posts(status, published_at);

create table reader_links (
    token text primary key,
    contact_id integer not null references contacts(id) on delete cascade,
    expires_at integer not null
);
