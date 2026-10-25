-- Each object's records move to a table of their own (o_<object>, a column
-- per field, f_<field>; see lib/store.qwe), made and changed as the object
-- is designed. A field can be unique: its table has an index that says so.

alter table custom_fields add column is_unique integer not null default 0;
