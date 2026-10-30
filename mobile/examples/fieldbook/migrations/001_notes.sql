-- Field notes: what was seen, where, when, with a photo and a voice memo.
create table notes (
    id integer primary key,
    title text not null,
    body text not null default '',
    lat real, lon real, accuracy real,
    place_from text,            -- "web" (the web view's geolocation) or "native" (the app's)
    heading real,               -- degrees from north, when the compass gave one
    photo text, thumb text,     -- paths under media/
    audio text,
    weather text,               -- JSON from open-meteo, fetched once
    done integer not null default 0,
    created integer not null
);
create table beats (name text primary key, at integer not null, n integer not null default 0);
create table probes (id integer primary key, at integer not null, rows text not null);
