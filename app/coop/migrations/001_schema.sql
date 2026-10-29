-- Quillo Coop: a credit cooperative. Members save, hold share capital, open
-- fixed deposits, borrow under loan products the manager designs and the board
-- approves, buy in the coop's stores, join raffles and events, and pay through
-- salary deductions their employers remit each month.
--
-- Amounts are whole centavos (integer); rates are basis points a year (1200 is
-- 12%). Days are 'YYYY-MM-DD' in the coop's time zone; moments (created, at,
-- expires) are milliseconds since 1970, as now() gives them.

create table employers (
    id integer primary key,
    code text not null unique collate nocase,       -- "ACME"
    name text not null,
    contact text not null default '',
    api_key_hash text,                              -- for POST /api/remittances
    payroll_day integer not null default 15
);

create table members (
    id integer primary key,
    number text unique,                             -- "M-0001", given on approval
    first_name text not null,
    last_name text not null,
    birth_date text not null,
    address text not null,
    phone text not null,
    email text not null,
    employer_id integer references employers(id),
    employee_no text,
    monthly_savings integer not null default 0,     -- taken from each salary
    pledge integer not null default 0,              -- share capital promised on joining
    status text not null check (status in ('applied', 'active', 'rejected', 'resigned', 'suspended')),
    applied_at integer not null,
    decided_at integer,
    decided_by integer,
    note text not null default '',
    unique (employer_id, employee_no)
);

create table users (
    id integer primary key,
    username text not null unique collate nocase,
    name text not null,
    role text not null check (role in ('member', 'clerk', 'bod', 'manager', 'employer')),
    password_hash text not null,
    member_id integer references members(id),
    employer_id integer references employers(id),
    must_change integer not null default 0,
    created integer not null
);

create table sessions (
    id text primary key,
    user_id integer not null references users(id),
    expires integer not null
);

-- A member's accounts: one savings and one share capital account, a deposit
-- account for each fixed deposit, and a loan account for each released loan
-- (its balance is what is still owed, a positive number).
create table accounts (
    id integer primary key,
    member_id integer not null references members(id),
    kind text not null check (kind in ('savings', 'share', 'deposit', 'loan')),
    number text not null unique,
    status text not null default 'open' check (status in ('open', 'closed', 'matured')),
    balance integer not null default 0,
    opened integer not null,
    term_months integer,                            -- deposits
    rate_bp integer,                                -- deposits
    starts text,                                    -- deposits
    matures text,                                   -- deposits
    principal integer,                              -- deposits
    loan_id integer                                 -- loans
);

-- Every movement on an account. A journal groups the entries of one posting
-- (a transfer is two). amount is signed: + adds to the balance.
create table entries (
    id integer primary key,
    account_id integer not null references accounts(id),
    journal text not null,
    amount integer not null,
    balance integer not null,
    kind text not null,
    memo text not null default '',
    by_user integer,
    at integer not null
);
create index entries_by_account on entries (account_id, id);

-- Loan products, made in the loan designer and approved by the board.
create table products (
    id integer primary key,
    code text not null unique collate nocase,
    name text not null,
    description text not null default '',
    min_amount integer not null,
    max_amount integer not null,
    min_term integer not null,                      -- months
    max_term integer not null,
    rate_bp integer not null,
    method text not null check (method in ('diminishing', 'flat')),
    fee_bp integer not null default 0,              -- service fee, of the principal, taken at release
    insurance_bp integer not null default 0,        -- loan insurance, likewise
    capital_multiple integer not null default 0,    -- the most one may borrow: this many times share capital and savings (0: no limit)
    min_months integer not null default 0,          -- months of membership needed
    comakers integer not null default 0,            -- members who must vouch for the borrower
    max_active integer not null default 1,          -- open loans of this product a member may have
    salary_only integer not null default 0,         -- paid through salary deductions only
    approvals integer not null default 2,           -- board approvals a loan needs
    status text not null default 'draft' check (status in ('draft', 'proposed', 'active', 'retired')),
    designed_by integer references users(id),
    proposed_at integer,
    decided_at integer
);

create table product_votes (
    product_id integer not null references products(id),
    director_id integer not null references users(id),
    vote text not null check (vote in ('approve', 'reject')),
    comment text not null default '',
    at integer not null,
    primary key (product_id, director_id)
);

create table loans (
    id integer primary key,
    number text not null unique,                    -- "L-00012"
    member_id integer not null references members(id),
    product_id integer not null references products(id),
    amount integer not null,
    term integer not null,
    rate_bp integer not null,                       -- the product's, when applied for
    method text not null,
    fee integer not null,
    insurance integer not null,
    monthly integer not null,                       -- the first installment
    purpose text not null,
    repay text not null check (repay in ('salary', 'counter')),
    status text not null check (status in ('submitted', 'board', 'approved', 'rejected', 'returned', 'released', 'paid', 'cancelled')),
    applied_at integer not null,
    reviewed_by integer,
    review_note text not null default '',
    forwarded_at integer,
    decided_at integer,
    released_at integer,
    released_by integer,
    first_due text
);

create table comakers (
    loan_id integer not null references loans(id),
    member_id integer not null references members(id),
    status text not null default 'asked' check (status in ('asked', 'agreed', 'declined')),
    at integer,
    primary key (loan_id, member_id)
);

create table loan_votes (
    loan_id integer not null references loans(id),
    director_id integer not null references users(id),
    vote text not null check (vote in ('approve', 'reject')),
    comment text not null default '',
    at integer not null,
    primary key (loan_id, director_id)
);

-- A released loan's installments, and what has been paid of each.
create table installments (
    loan_id integer not null references loans(id),
    n integer not null,
    due text not null,
    principal integer not null,
    interest integer not null,
    paid integer not null default 0,
    primary key (loan_id, n)
);

-- Salary deductions: each month, a batch for each employer lists what to take
-- from each employed member's salary; the employer remits it, and the
-- remittance is posted line by line.
create table batches (
    id integer primary key,
    employer_id integer not null references employers(id),
    period text not null,                           -- "2026-11"
    status text not null default 'open' check (status in ('open', 'posted')),
    created integer not null,
    posted_at integer,
    unique (employer_id, period)
);

create table batch_lines (
    id integer primary key,
    batch_id integer not null references batches(id),
    member_id integer not null references members(id),
    kind text not null check (kind in ('loan', 'savings', 'store')),
    ref_id integer,                                 -- the loan's or the order's id
    amount integer not null,
    remitted integer not null default 0,
    status text not null default 'due' check (status in ('due', 'paid', 'short', 'missing'))
);

create table remittances (
    id integer primary key,
    batch_id integer not null references batches(id),
    source text not null,                           -- "upload: march.csv", "api"
    lines integer not null,
    total integer not null,
    by_user integer,
    at integer not null
);

-- The coop's web stores and what they sell.
create table stores (
    id integer primary key,
    slug text not null unique,
    name text not null,
    description text not null default ''
);

create table items (
    id integer primary key,
    store_id integer not null references stores(id),
    sku text not null unique,
    name text not null,
    description text not null default '',
    price integer not null,
    member_price integer not null,
    stock integer not null,
    active integer not null default 1
);

create table orders (
    id integer primary key,
    number text not null unique,
    member_id integer not null references members(id),
    store_id integer not null references stores(id),
    total integer not null,
    pay text not null check (pay in ('savings', 'salary', 'pickup')),
    installments integer not null default 1,
    paid integer not null default 0,                -- of the total, for orders paid through salary
    status text not null default 'placed' check (status in ('placed', 'ready', 'collected', 'cancelled')),
    at integer not null
);

create table order_lines (
    order_id integer not null references orders(id),
    item_id integer not null references items(id),
    qty integer not null,
    price integer not null,
    primary key (order_id, item_id)
);

-- Raffles: tickets bought from savings; the draw takes a seed whose hash was
-- shown when the raffle opened, so anyone can check the winner was not picked.
create table raffles (
    id integer primary key,
    name text not null,
    prize text not null,
    ticket_price integer not null,
    max_tickets integer not null,
    per_member integer not null,
    closes text not null,
    status text not null default 'open' check (status in ('open', 'closed', 'drawn')),
    seed_hash text not null,
    seed text,                                      -- shown once drawn
    winner_ticket integer,
    drawn_at integer,
    drawn_by integer
);

create table raffle_tickets (
    id integer primary key,
    raffle_id integer not null references raffles(id),
    n integer not null,
    member_id integer not null references members(id),
    at integer not null,
    unique (raffle_id, n)
);

create table events (
    id integer primary key,
    slug text not null unique,
    title text not null,
    description text not null default '',
    starts text not null,                           -- "2026-11-20T09:00"
    venue text not null,
    capacity integer not null,
    price integer not null default 0,
    status text not null default 'open' check (status in ('open', 'closed', 'cancelled'))
);

create table event_tickets (
    id integer primary key,
    event_id integer not null references events(id),
    member_id integer not null references members(id),
    code text not null unique,
    status text not null default 'booked' check (status in ('booked', 'checked_in', 'cancelled')),
    at integer not null,
    checked_at integer,
    checked_by integer,
    unique (event_id, member_id)
);

-- The help desk: a member's questions and the clerks' answers.
create table tickets (
    id integer primary key,
    member_id integer not null references members(id),
    subject text not null,
    status text not null default 'open' check (status in ('open', 'answered', 'closed')),
    at integer not null,
    updated integer not null
);

create table ticket_messages (
    id integer primary key,
    ticket_id integer not null references tickets(id),
    user_id integer not null references users(id),
    body text not null,
    at integer not null
);

create table settings (
    key text primary key,
    value text not null
);

create table audit (
    id integer primary key,
    user_id integer,
    action text not null,
    detail text not null default '',
    at integer not null
);
