-- The live preview: what a page's designer has on screen, saved as it
-- changes (saved as a draft or not), and each page's secret link to it.
create table page_live (
    page_id integer primary key references pages(id) on delete cascade,
    blocks text not null,
    focus text not null default '',           -- the block being edited
    user_id integer references users(id) on delete set null,
    updated_at integer not null
);
alter table pages add column preview_token text not null default '';
