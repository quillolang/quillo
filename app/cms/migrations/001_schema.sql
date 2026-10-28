-- Newsroom: people and their roles, flows (steps and transitions), articles
-- and pages made of blocks, review threads on blocks, and the objects a
-- designer makes: each object's records live in a table of its own,
-- obj_<id>, made and altered at run time (lib/objects.qwe).

create table users (
    id integer primary key,
    name text not null unique collate nocase,
    email text not null unique collate nocase,
    password_hash text not null,
    hue integer not null default 210,
    created integer not null
);

-- admin, designer, writer, reviewer, publisher: someone may have several.
create table user_roles (
    user_id integer not null references users(id) on delete cascade,
    role text not null,
    primary key (user_id, role)
);

create table sessions (
    id text primary key,
    user_id integer not null references users(id) on delete cascade,
    created integer not null,
    expires integer not null
);

-- A flow is a state machine: articles move through one, and so does each
-- object's records. kind: article or object.
create table flows (
    id integer primary key,
    name text not null,
    kind text not null default 'article',
    created integer not null
);

-- role: who acts while something is at this step (null: nobody, as at the
-- end). approvals: how many different people with the role must approve.
create table steps (
    id integer primary key,
    flow_id integer not null references flows(id) on delete cascade,
    name text not null,
    position integer not null default 0,
    role text,
    approvals integer not null default 1,
    color text not null default '#64748b'
);

-- kind: approve (counts approvals until the step's number is reached), reject
-- (sends back at once, and approvals start over), move (at once).
-- needs_resolved: not while feedback on the article is open.
create table transitions (
    id integer primary key,
    flow_id integer not null references flows(id) on delete cascade,
    from_step integer not null references steps(id) on delete cascade,
    to_step integer not null references steps(id) on delete cascade,
    name text not null,
    role text not null,
    kind text not null default 'move',
    needs_resolved integer not null default 0,
    position integer not null default 0
);

create table articles (
    id integer primary key,
    title text not null,
    slug text not null unique,
    flow_id integer not null references flows(id),
    step_id integer not null references steps(id),
    author_id integer not null references users(id),
    round integer not null default 1,          -- goes up each time it is sent back
    created integer not null,
    updated integer not null,
    published_at integer
);

create table approvals (
    article_id integer not null references articles(id) on delete cascade,
    step_id integer not null,
    round integer not null,
    user_id integer not null references users(id),
    at integer not null,
    primary key (article_id, step_id, round, user_id)
);

create table article_log (
    id integer primary key,
    article_id integer not null references articles(id) on delete cascade,
    user_id integer not null references users(id),
    at integer not null,
    text text not null
);
create index article_log_by_article on article_log (article_id, id);

-- owner: 'article:<id>' or 'page:<id>'. data: JSON, by type.
-- version goes up with each change, so feedback can say "changed since".
create table blocks (
    id integer primary key,
    owner text not null,
    position integer not null default 0,
    type text not null,
    data text not null default '{}',
    version integer not null default 1,
    updated integer not null,
    updated_by integer references users(id)
);
create index blocks_by_owner on blocks (owner, position);

create table threads (
    id integer primary key,
    article_id integer not null references articles(id) on delete cascade,
    block_id integer references blocks(id) on delete set null,
    user_id integer not null references users(id),
    created integer not null,
    status text not null default 'open',
    closed_by integer references users(id),
    closed_at integer,
    block_version integer not null default 1
);
create index threads_by_article on threads (article_id, status);

create table posts (
    id integer primary key,
    thread_id integer not null references threads(id) on delete cascade,
    user_id integer not null references users(id),
    body text not null,
    created integer not null
);

-- Who has an article open, and on which block: kept apart from the article,
-- so a heartbeat wakes only who shows presence.
create table presence (
    article_id integer not null references articles(id) on delete cascade,
    user_id integer not null references users(id) on delete cascade,
    block_id integer,
    seen integer not null,
    primary key (article_id, user_id)
);

-- ---- what a designer makes --------------------------------------------------

create table objects (
    id integer primary key,
    name text not null unique collate nocase,
    flow_id integer not null references flows(id),
    created integer not null,
    created_by integer not null references users(id)
);

-- Each field is the column f_<id> of the object's table.
-- type: text, longtext, number, money, date, choice, yesno, user.
create table fields (
    id integer primary key,
    object_id integer not null references objects(id) on delete cascade,
    name text not null,
    type text not null,
    required integer not null default 0,
    options text not null default '[]',
    position integer not null default 0
);

-- kind create: makes a record at the flow's first step, for `role`.
-- kind transaction: runs `transition_id`, for the transition's role.
-- fields: the ids of the fields the form asks for, as JSON.
create table forms (
    id integer primary key,
    object_id integer not null references objects(id) on delete cascade,
    name text not null,
    kind text not null,
    transition_id integer references transitions(id) on delete cascade,
    role text,
    fields text not null default '[]',
    created integer not null
);

-- columns, states: JSON lists of field and step ids (no states: all).
create table lists (
    id integer primary key,
    object_id integer not null references objects(id) on delete cascade,
    name text not null,
    columns text not null default '[]',
    states text not null default '[]',
    sort_field integer,
    sort_desc integer not null default 1
);

-- Every record's making and every transaction on it.
create table record_log (
    id integer primary key,
    object_id integer not null references objects(id) on delete cascade,
    record_id integer not null,
    form_id integer,
    from_step integer,
    to_step integer not null,
    user_id integer not null references users(id),
    at integer not null,
    data text not null default '{}'
);
create index record_log_by_record on record_log (object_id, record_id, id);

-- Pages a designer builds from blocks. roles: who may see it (JSON; empty: everyone signed in).
create table pages (
    id integer primary key,
    title text not null,
    slug text not null unique,
    roles text not null default '[]',
    created integer not null
);
