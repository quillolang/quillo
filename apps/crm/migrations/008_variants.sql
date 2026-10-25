-- Products in variants (sizes, colours): each with its stock and, if it
-- differs, its price.

alter table products add column variant_label text not null default '';   -- "Size"
alter table products add column variants text not null default '[]';      -- [{ name, stock, price_cents }]
