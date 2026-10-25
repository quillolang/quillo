-- Pages a workspace designs: landing pages, a home page, made of blocks.

create table pages (
    id integer primary key,
    slug text not null unique,
    title text not null,
    description text not null default '',
    draft text not null default '{"blocks": []}',      -- what the designer edits
    published text,                                   -- what visitors see (null: never published)
    home integer not null default 0,                  -- shown at the workspace's own address
    created_at integer not null,
    updated_at integer not null,
    published_at integer
);
