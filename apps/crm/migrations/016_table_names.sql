-- An object's table is named when the object is made (`o_press_releases`),
-- and can be renamed; empty is the name from before, `o_<key>`.
alter table objects add column table_name text not null default '';
