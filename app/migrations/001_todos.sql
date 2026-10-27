-- The to-do list.

create table todos (
    id integer primary key,
    text text not null,
    done integer not null default 0
);
