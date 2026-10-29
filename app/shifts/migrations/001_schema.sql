-- Quillo Shifts: a factory's production lines, the three crews of each line,
-- who is in which crew and from when, leave and who decided it, and sessions.
--
-- Days are 'YYYY-MM-DD' text in the factory's time zone (FACTORY_TZ); moments
-- (created, decided, expires) are milliseconds since 1970, as now() gives them.
-- A night shift belongs to the day it starts on: the night of the 24th runs
-- from 19:00 on the 24th to 07:00 on the 25th.

create table lines (
    id integer primary key,
    code text not null unique,                 -- "L01"
    name text not null,
    area text not null,                        -- "Assembly", "Press", ...
    min_day integer not null,                  -- the fewest operators a day shift may run with
    min_night integer not null
);

-- Each line has crews A, B and C. A crew's offset is where it starts in the
-- six-day rotation (D D N N R R): A at 0, B at 2, C at 4, so on any day one
-- crew is on days, one on nights and one resting.
create table crews (
    id integer primary key,
    line_id integer not null references lines(id),
    letter text not null check (letter in ('A', 'B', 'C')),
    offset integer not null,
    unique (line_id, letter)
);

create table users (
    id integer primary key,
    emp_no text not null unique collate nocase,   -- "E0001", "S01", "M01"
    name text not null,
    role text not null check (role in ('operator', 'supervisor', 'manager')),
    password_hash text not null,
    annual_days integer not null default 14,      -- shifts of annual leave a year
    hue integer not null default 210
);

-- Who is in a crew, from a day to a day (to_day null: still there). An
-- operator moved to another crew has two rows; the days do not overlap.
create table members (
    id integer primary key,
    user_id integer not null references users(id),
    crew_id integer not null references crews(id),
    from_day text not null,
    to_day text,
    check (to_day is null or to_day >= from_day)
);
create index members_by_crew on members (crew_id);
create index members_by_user on members (user_id);

create table supervises (
    user_id integer not null references users(id),
    line_id integer not null references lines(id),
    primary key (user_id, line_id)
);

-- A request covers the days from start_day to end_day; `shifts` is how many of
-- them the operator's crew works (rest days are not leave), worked out when
-- it was asked for.
create table leaves (
    id integer primary key,
    user_id integer not null references users(id),
    kind text not null check (kind in ('annual', 'sick', 'unpaid')),
    start_day text not null,
    end_day text not null,
    shifts integer not null,
    note text not null default '',
    status text not null default 'pending' check (status in ('pending', 'approved', 'rejected', 'cancelled')),
    decided_by integer references users(id),
    decided_at integer,
    reason text not null default '',
    created integer not null,
    check (end_day >= start_day)
);
create index leaves_by_user on leaves (user_id, start_day);
create index leaves_by_days on leaves (start_day, end_day);

create table sessions (
    id text primary key,
    user_id integer not null references users(id),
    expires integer not null
);
