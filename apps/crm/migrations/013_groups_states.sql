-- Groups of people (with roles, and what they are on the team: designers,
-- writers, reviewers, publishers); objects' states and the moves between
-- them; and the tasks flows give a group or a role.

create table groups (
    id integer primary key,
    name text not null unique,
    members text not null default '[]',        -- user ids
    roles text not null default '[]',          -- role ids, which members get
    capabilities text not null default '[]',   -- designer, writer, reviewer, publisher
    created_at integer not null
);

alter table objects add column states text not null default '[]';        -- [{ key, label, color }], the first is where records start
alter table objects add column transitions text not null default '[]';   -- [{ key, label, from: [states] (empty: any), to, groups: [ids], roles: [ids] }]
alter table object_records add column state text not null default '';

alter table desks add column editor_groups text not null default '[]';
alter table desks add column reviewer_groups text not null default '[]';
alter table desks add column publisher_groups text not null default '[]';

create table flow_tasks (
    id integer primary key,
    run_id integer not null references flow_runs(id) on delete cascade,
    title text not null,
    details text not null default '',
    group_id integer references groups(id) on delete set null,
    role_id integer references roles(id) on delete set null,
    object text not null default '',
    record_id integer,
    contact_id integer,
    status text not null default 'open',       -- open, done
    outcome text not null default '',          -- approved, rejected
    note text not null default '',
    done_by integer references users(id) on delete set null,
    created_at integer not null,
    done_at integer
);
create index flow_tasks_open on flow_tasks(status, created_at);
