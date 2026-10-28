-- Quillo Stays: people, places to stay and their calendars, bookings (one row
-- a night in booking_nights, so two bookings can never share a night),
-- reviews, activities and their time slots, and what the outside services
-- (maps, weather, places) answered, kept for a while.

create table users (
    id integer primary key,
    email text not null unique collate nocase,
    name text not null,
    password_hash text not null,
    hosts integer not null default 0,              -- 1 when they host places
    hue integer not null default 200,
    created integer not null
);

create table listings (
    id integer primary key,
    host_id integer not null references users(id),
    title text not null,
    kind text not null,                            -- apartment, house, cabin, room
    city text not null,
    country text not null,
    address text not null default '',
    lat real not null,
    lng real not null,
    currency text not null,
    base_price integer not null,                   -- a night, in minor units
    weekend_price integer not null,                -- Friday and Saturday nights
    cleaning_fee integer not null default 0,
    max_guests integer not null,
    bedrooms integer not null default 1,
    beds integer not null default 1,
    baths integer not null default 1,
    amenities text not null default '',            -- comma-separated keys
    description text not null default '',
    instant_book integer not null default 1,       -- else the host accepts each request
    cancel_policy text not null default 'moderate', -- flexible, moderate or strict
    min_nights integer not null default 1,
    max_nights integer not null default 30,
    check_in text not null default '15:00',
    check_out text not null default '11:00',
    hue integer not null default 200,              -- the colour of its pictures
    photos integer not null default 5,
    active integer not null default 1,
    created integer not null
);
create index listings_by_place on listings (active, lat, lng);
create index listings_by_host on listings (host_id);

-- A price the host set for one night, over the usual ones.
create table night_prices (
    listing_id integer not null references listings(id),
    day text not null,                             -- YYYY-MM-DD
    price integer not null,
    primary key (listing_id, day)
);

-- Nights the host closed.
create table blocked_nights (
    listing_id integer not null references listings(id),
    day text not null,
    primary key (listing_id, day)
);

create table bookings (
    id integer primary key,
    listing_id integer not null references listings(id),
    guest_id integer not null references users(id),
    start_day text not null,                       -- the first night
    end_day text not null,                         -- the day they leave
    guests integer not null,
    status text not null,                          -- held, requested, confirmed, declined, cancelled, expired
    held_until integer,                            -- a hold's end, while the guest pays
    respond_by integer,                            -- a request's end, for the host
    currency text not null,
    total integer not null,
    breakdown text not null default '{}',          -- the quote, as JSON
    card_last4 text,
    refund integer,
    created integer not null,
    decided integer
);
create index bookings_by_guest on bookings (guest_id, id);
create index bookings_by_listing on bookings (listing_id, start_day);

-- The nights a booking holds: one row each, unique for a place and a day, so
-- the database itself refuses a second booking of the same night.
create table booking_nights (
    booking_id integer not null references bookings(id),
    listing_id integer not null,
    day text not null,
    primary key (listing_id, day)
);
create index booking_nights_by_booking on booking_nights (booking_id);

create table reviews (
    id integer primary key,
    booking_id integer not null unique references bookings(id),
    listing_id integer not null references listings(id),
    guest_id integer not null references users(id),
    rating integer not null,
    body text not null,
    created integer not null
);
create index reviews_by_listing on reviews (listing_id, id);

create table saved (
    user_id integer not null references users(id),
    listing_id integer not null references listings(id),
    created integer not null,
    primary key (user_id, listing_id)
);

create table activities (
    id integer primary key,
    title text not null,
    kind text not null,                            -- food, water, culture, outdoors
    city text not null,
    lat real not null,
    lng real not null,
    currency text not null,
    price integer not null,                        -- a person
    minutes integer not null,
    description text not null default '',
    hue integer not null default 30
);

create table activity_slots (
    id integer primary key,
    activity_id integer not null references activities(id),
    day text not null,
    time text not null,                            -- HH:MM, local
    capacity integer not null,
    taken integer not null default 0
);
create index activity_slots_by_day on activity_slots (activity_id, day);

create table activity_bookings (
    id integer primary key,
    slot_id integer not null references activity_slots(id),
    user_id integer not null references users(id),
    people integer not null,
    total integer not null,
    currency text not null,
    status text not null default 'confirmed',
    created integer not null
);

-- What maps, weather and places services answered, by question, for a while.
create table outside (
    key text primary key,
    service text not null,
    body text not null,
    fetched integer not null
);

-- Each call to an outside service: for the hosts' status page and for
-- keeping to their limits (Nominatim: one a second).
create table outside_calls (
    id integer primary key,
    service text not null,
    url text not null,
    status integer not null,
    ms integer not null,
    created integer not null
);
create index outside_calls_by_service on outside_calls (service, id);
