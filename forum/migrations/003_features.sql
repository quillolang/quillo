-- Following threads, bookmarks, what each person has read, reactions,
-- reports and the moderators' log, bans, private messages, polls and images.

-- Following a thread notifies of every reply to it. Starting a thread or
-- replying follows it; unfollowing mutes it.
create table follows (
    user_id integer not null references users(id),
    thread_id integer not null references threads(id),
    created integer not null,
    primary key (user_id, thread_id)
);
create index follows_by_thread on follows (thread_id);

insert or ignore into follows (user_id, thread_id, created)
    select distinct p.user_id, p.thread_id, min(p.created) from posts p group by p.user_id, p.thread_id;

create table bookmarks (
    user_id integer not null references users(id),
    post_id integer not null references posts(id),
    created integer not null,
    primary key (user_id, post_id)
);

-- The last post each person has seen in each thread; posts after it are unread.
create table reads (
    user_id integer not null references users(id),
    thread_id integer not null references threads(id),
    last_post_id integer not null,
    primary key (user_id, thread_id)
);

create table reactions (
    post_id integer not null references posts(id),
    user_id integer not null references users(id),
    emoji text not null,
    created integer not null,
    primary key (post_id, user_id, emoji)
);

-- A post someone flagged. Open until a moderator dismisses it or removes the post.
create table reports (
    id integer primary key,
    post_id integer not null references posts(id),
    user_id integer not null references users(id),
    reason text not null,                         -- spam, rude, off-topic or other
    note text not null default '',
    created integer not null,
    status text not null default 'open',          -- open, dismissed or removed
    resolved_by integer references users(id),
    resolved_at integer,
    unique (post_id, user_id)
);
create index reports_by_status on reports (status, id);

-- What moderators did, newest last: removals, bans, pins, locks, roles.
create table mod_log (
    id integer primary key,
    actor_id integer not null references users(id),
    action text not null,
    user_id integer references users(id),         -- whom it was about
    thread_id integer,
    post_id integer,
    note text not null default '',
    created integer not null
);

-- Banned people may read but not post, vote, react or send messages.
alter table users add column banned_until integer;

-- One conversation for each pair of people, a_id < b_id.
create table conversations (
    id integer primary key,
    a_id integer not null references users(id),
    b_id integer not null references users(id),
    created integer not null,
    last_at integer not null,
    unique (a_id, b_id)
);

create table messages (
    id integer primary key,
    conversation_id integer not null references conversations(id),
    user_id integer not null references users(id),
    body text not null,
    html text not null,
    created integer not null
);
create index messages_by_conversation on messages (conversation_id, id);

create table message_reads (
    user_id integer not null references users(id),
    conversation_id integer not null references conversations(id),
    last_id integer not null,
    primary key (user_id, conversation_id)
);

-- A thread's poll: one or several choices, open until closes_at (or for good).
create table polls (
    thread_id integer primary key references threads(id),
    question text not null,
    multiple integer not null default 0,
    closes_at integer
);

create table poll_options (
    id integer primary key,
    thread_id integer not null references polls(thread_id),
    position integer not null,
    text text not null
);

create table poll_votes (
    thread_id integer not null references polls(thread_id),
    option_id integer not null references poll_options(id),
    user_id integer not null references users(id),
    created integer not null,
    primary key (option_id, user_id)
);
create index poll_votes_by_thread on poll_votes (thread_id, user_id);

-- Images people uploaded, each shrunk and saved under uploads/.
create table uploads (
    id integer primary key,
    user_id integer not null references users(id),
    name text not null unique,                    -- the file under uploads/images/
    width integer not null,
    height integer not null,
    bytes integer not null,
    created integer not null
);
create index uploads_by_user on uploads (user_id, created);
