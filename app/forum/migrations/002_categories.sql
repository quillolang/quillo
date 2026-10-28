-- The categories a new forum starts with.

insert into categories (slug, name, about, color, position) values
    ('announcements', 'Announcements', 'News about the forum and the project.', '#f59e0b', 1),
    ('general', 'General', 'Anything that does not fit elsewhere.', '#6366f1', 2),
    ('help', 'Help', 'Stuck? Ask here, and mark the answer that solved it.', '#10b981', 3),
    ('show-and-tell', 'Show and tell', 'Things you built, and how.', '#ec4899', 4),
    ('ideas', 'Ideas', 'Proposals, wishes and what should come next.', '#0ea5e9', 5);
