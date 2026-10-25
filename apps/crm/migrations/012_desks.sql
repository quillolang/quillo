-- Editorial desks: for each category of posts, who writes and edits, who
-- reviews, who publishes, and how many approvals a post needs; and the
-- reviews posts get on their way.

create table desks (
    id integer primary key,
    category text not null unique,           -- a posts' category; '' for every category without a desk
    editors text not null default '[]',      -- user ids
    reviewers text not null default '[]',
    publishers text not null default '[]',
    approvals integer not null default 1,
    created_at integer not null
);

alter table posts add column stage text not null default 'draft';   -- draft, review, changes, approved, published
alter table posts add column submitted_at integer;                   -- when this version went for review

create table post_reviews (
    id integer primary key,
    post_id integer not null references posts(id) on delete cascade,
    user_id integer references users(id) on delete set null,
    decision text not null,                  -- approved, changes, submitted, published
    note text not null default '',
    created_at integer not null
);
create index post_reviews_post on post_reviews(post_id, created_at);
