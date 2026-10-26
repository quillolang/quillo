-- Many objects and pages: folders to keep them in, pins (in everyone's
-- sidebar), and what each person opened lately (in theirs).
alter table objects add column folder text not null default '';
alter table objects add column pinned integer not null default 0;
alter table pages add column folder text not null default '';
alter table pages add column pinned integer not null default 0;
create table recents (
    user_id integer not null references users(id) on delete cascade,
    kind text not null,                 -- object, page
    ref text not null,                  -- an object's key, a page's id
    at integer not null,
    primary key (user_id, kind, ref)
);
