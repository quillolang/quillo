-- A newsroom's blog: posts in categories (a news site's sections), featured
-- stories, how often each is read; authors with a line about them.

alter table posts add column category text not null default '';
alter table posts add column featured integer not null default 0;
alter table posts add column views integer not null default 0;
alter table users add column bio text not null default '';
create index posts_category on posts(category, status, published_at);
