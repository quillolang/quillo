-- Custom objects: kinds of records a workspace designs (vehicles, events,
-- tickets...). Their fields are in custom_fields (entity "obj.<key>"); their
-- rules, made in the designer, are data both sides run.

create table objects (
    id integer primary key,
    key text not null unique,                 -- vehicle
    name text not null,                       -- Vehicle
    plural text not null,                     -- Vehicles
    icon text not null default '◆',
    description text not null default '',
    title_field text not null default '',     -- the field whose value names a record
    rules text not null default '[]',         -- [{ if, then, message, warning }]
    created_at integer not null
);

create table object_records (
    id integer primary key,
    object text not null,                     -- objects.key
    title text not null default '',
    fields text not null default '{}',
    owner_id integer references users(id) on delete set null,
    created_at integer not null,
    updated_at integer not null
);
create index object_records_object on object_records(object, updated_at);
