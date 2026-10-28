-- The forum: people, their sessions, categories, threads, posts, votes, tags
-- and notifications.

create table users (
    id integer primary key,
    name text not null unique collate nocase,
    email text not null unique collate nocase,
    password_hash text not null,
    role text not null default 'member',          -- member, moderator or admin
    bio text not null default '',
    hue integer not null default 210,             -- the colour of their avatar
    created integer not null
);

-- Sessions live here, not only in the cookie, so signing out ends them for good.
create table sessions (
    id text primary key,
    user_id integer not null references users(id) on delete cascade,
    created integer not null,
    expires integer not null
);
create index sessions_by_user on sessions (user_id);

create table categories (
    id integer primary key,
    slug text not null unique,
    name text not null,
    about text not null default '',
    color text not null default '#6366f1',
    position integer not null default 0
);

create table threads (
    id integer primary key,
    category_id integer not null references categories(id),
    user_id integer not null references users(id),
    title text not null,
    created integer not null,
    last_post_at integer not null,
    last_user_id integer not null references users(id),
    replies integer not null default 0,
    pinned integer not null default 0,
    locked integer not null default 0,
    solution_id integer,
    deleted integer not null default 0
);
create index threads_by_category on threads (category_id, deleted, last_post_at);
create index threads_by_activity on threads (deleted, last_post_at);

-- Kept apart from threads, so counting a view wakes no live page.
create table thread_views (
    thread_id integer primary key references threads(id),
    n integer not null default 0
);

create table posts (
    id integer primary key,
    thread_id integer not null references threads(id),
    user_id integer not null references users(id),
    body text not null,                           -- Markdown, as written
    html text not null,                           -- what it renders to
    created integer not null,
    edited integer,
    deleted integer not null default 0
);
create index posts_by_thread on posts (thread_id, id);
create index posts_by_user on posts (user_id, id);

create table votes (
    post_id integer not null references posts(id),
    user_id integer not null references users(id),
    created integer not null,
    primary key (post_id, user_id)
);

create table tags (
    thread_id integer not null references threads(id),
    tag text not null,
    primary key (thread_id, tag)
);
create index tags_by_tag on tags (tag);

create table notifications (
    id integer primary key,
    user_id integer not null references users(id),
    kind text not null,                           -- reply, mention or solution
    actor_id integer not null references users(id),
    thread_id integer not null references threads(id),
    post_id integer,
    created integer not null,
    read integer not null default 0
);
create index notifications_by_user on notifications (user_id, read, id);

-- Full-text search over every post, with its thread's title; rowid is the post's id.
create virtual table search using fts5(title, body, tokenize = 'porter unicode61');

create trigger posts_search_insert after insert on posts begin
    insert into search (rowid, title, body)
        select new.id, t.title, new.body from threads t where t.id = new.thread_id;
end;

create trigger posts_search_update after update of body, deleted on posts begin
    delete from search where rowid = old.id;
    insert into search (rowid, title, body)
        select new.id, t.title, new.body from threads t where t.id = new.thread_id and new.deleted = 0;
end;
