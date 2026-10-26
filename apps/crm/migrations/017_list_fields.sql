-- The fields an object's list shows, in order (empty: the first few short ones).
alter table objects add column list_fields text not null default '[]';
