-- Objects on the site (an object's records read as articles, its
-- sections as another object's records), and review: comments on any
-- text or picture of a record, in threads, until resolved.

alter table objects add column site text not null default '{}';

create table annotations (
    id integer primary key,
    object text not null,
    record_id integer not null,
    field text not null,                     -- the field it is on
    quote text not null default '',          -- the text it marks ('' for a picture, or the whole field)
    prefix text not null default '',         -- a little of the text before, to find the right one
    image text not null default '',          -- the picture it is on (its src), if a picture
    body text not null,
    parent_id integer references annotations(id) on delete cascade,
    author_id integer references users(id) on delete set null,
    resolved_at integer,
    resolved_by integer references users(id) on delete set null,
    created_at integer not null
);
create index annotations_record on annotations(object, record_id, created_at);
