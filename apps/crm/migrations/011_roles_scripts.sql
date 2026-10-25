-- Roles for the team, with what each may do with each object (and a script
-- deciding which records they see); scripts on objects' fields, run on the
-- server when a record is saved.

create table roles (
    id integer primary key,
    name text not null unique,
    access text not null default '{}',     -- { <object>: { level: none | read | edit | delete, script } }
    created_at integer not null
);
alter table users add column role_id integer references roles(id) on delete set null;
alter table custom_fields add column script text not null default '';
