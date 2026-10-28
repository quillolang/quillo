-- Projects: people and their sessions, projects and who is in them, the
-- board's columns, labels, milestones, tasks and what hangs off a task, and
-- the history the reports are drawn from.
--
-- Days (start, due, a milestone's span) are 'YYYY-MM-DD' text; moments
-- (created, moved, done) are milliseconds since 1970, as now() gives them.

create table users (
    id integer primary key,
    name text not null unique collate nocase,
    email text not null unique collate nocase,
    password_hash text not null,
    hue integer not null default 210,
    created integer not null
);

create table sessions (
    id text primary key,
    user_id integer not null references users(id) on delete cascade,
    created integer not null,
    expires integer not null
);
create index sessions_by_user on sessions (user_id);

create table projects (
    id integer primary key,
    key text not null unique collate nocase,          -- "WEB": tasks are WEB-1, WEB-2, ...
    name text not null,
    about text not null default '',
    color text not null default '#4f46e5',
    next_num integer not null default 1,
    created integer not null,
    archived integer not null default 0
);

-- owner: everything; editor: tasks; viewer: reads.
create table members (
    project_id integer not null references projects(id) on delete cascade,
    user_id integer not null references users(id) on delete cascade,
    role text not null default 'editor',
    primary key (project_id, user_id)
);
create index members_by_user on members (user_id);

-- kind says what a column means to the reports: todo, doing or done.
create table columns (
    id integer primary key,
    project_id integer not null references projects(id) on delete cascade,
    name text not null,
    kind text not null default 'todo',
    position integer not null default 0,
    wip integer                                     -- a limit on cards in it, or null
);
create index columns_by_project on columns (project_id, position);

create table labels (
    id integer primary key,
    project_id integer not null references projects(id) on delete cascade,
    name text not null,
    color text not null
);

create table milestones (
    id integer primary key,
    project_id integer not null references projects(id) on delete cascade,
    name text not null,
    start_day text not null,
    due_day text not null
);

create table tasks (
    id integer primary key,
    project_id integer not null references projects(id) on delete cascade,
    num integer not null,
    title text not null,
    body text not null default '',
    column_id integer not null references columns(id),
    position integer not null default 0,
    priority integer not null default 0,           -- 0 none, 1 low, 2 medium, 3 high, 4 urgent
    assignee_id integer references users(id) on delete set null,
    points integer,
    start_day text,
    due_day text,
    milestone_id integer references milestones(id) on delete set null,
    created integer not null,
    created_by integer not null references users(id),
    updated integer not null,
    started_at integer,                             -- first time it reached a 'doing' column
    done_at integer,                                -- when it last reached a 'done' column
    unique (project_id, num)
);
create index tasks_by_column on tasks (column_id, position);
create index tasks_by_project on tasks (project_id);

create table task_labels (
    task_id integer not null references tasks(id) on delete cascade,
    label_id integer not null references labels(id) on delete cascade,
    primary key (task_id, label_id)
);

-- task_id cannot be done until blocked_by is.
create table deps (
    task_id integer not null references tasks(id) on delete cascade,
    blocked_by integer not null references tasks(id) on delete cascade,
    primary key (task_id, blocked_by),
    check (task_id != blocked_by)
);

create table checklist (
    id integer primary key,
    task_id integer not null references tasks(id) on delete cascade,
    text text not null,
    done integer not null default 0,
    position integer not null default 0
);

create table comments (
    id integer primary key,
    task_id integer not null references tasks(id) on delete cascade,
    user_id integer not null references users(id),
    body text not null,
    created integer not null
);
create index comments_by_task on comments (task_id, id);

-- Every time a task enters a column (made there, or moved): the cumulative
-- flow, burndown, cycle times and throughput are worked out from these.
create table moves (
    id integer primary key,
    project_id integer not null references projects(id) on delete cascade,
    task_id integer not null references tasks(id) on delete cascade,
    column_id integer not null references columns(id),
    at integer not null
);
create index moves_by_project on moves (project_id, at);

create table activity (
    id integer primary key,
    project_id integer not null references projects(id) on delete cascade,
    task_id integer references tasks(id) on delete set null,
    user_id integer not null references users(id),
    at integer not null,
    text text not null
);
create index activity_by_project on activity (project_id, id);
