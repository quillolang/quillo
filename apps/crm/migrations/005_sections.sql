-- Sections: pieces of pages, made in the designer like a page, that pages
-- put in with the Section block. A section has no address of its own.

alter table pages add column kind text not null default 'page';    -- page, section
