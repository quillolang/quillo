-- Feedback on a page's live preview: a block as it was (rendered by the
-- server), what the viewer drew on it and said; the team's to-dos, talked
-- over by both sides, resolved by the viewer.
create table page_feedback (
    id integer primary key,
    page_id integer not null references pages(id) on delete cascade,
    block_id text not null,
    block_html text not null,          -- the block, rendered when the feedback was given
    width integer not null,            -- its size then, which the drawing is measured in
    height integer not null,
    strokes text not null default '[]',   -- [{ color, points: [[x, y], ...] }]
    body text not null,
    author text not null,              -- the viewer's name, as they gave it
    status text not null default 'open',   -- open, acknowledged, resolved
    acknowledged_by integer references users(id) on delete set null,
    resolved_at integer,
    created_at integer not null
);
create index page_feedback_page on page_feedback(page_id, created_at);
create table page_feedback_replies (
    id integer primary key,
    feedback_id integer not null references page_feedback(id) on delete cascade,
    body text not null,
    author text not null,
    user_id integer references users(id) on delete set null,   -- one of the team; null: the viewer
    created_at integer not null
);
create index page_feedback_replies_of on page_feedback_replies(feedback_id, created_at);
