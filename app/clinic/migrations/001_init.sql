create table branches (
  id integer primary key,
  code text not null unique,
  name text not null,
  zone text not null,
  locale text not null,
  currency text not null,
  tax_rate decimal_text not null,
  opens text not null,
  closes text not null
);
create table users (
  id integer primary key,
  email text not null unique,
  name text not null,
  password_hash text not null,
  role text not null default 'patient',
  locale text,
  zone text,
  branch_id integer references branches(id)
);
create table doctors (
  id integer primary key,
  user_id integer not null references users(id),
  branch_id integer not null references branches(id),
  specialty text not null,
  fee decimal_text not null,
  slot_minutes integer not null default 30,
  days text not null default '1,2,3,4,5'
);
create table appointments (
  id integer primary key,
  doctor_id integer not null references doctors(id),
  patient_id integer not null references users(id),
  starts_at text not null,
  ends_at text not null,
  status text not null default 'booked',
  reason text not null default '',
  created_at text not null
);
create unique index appointments_slot on appointments(doctor_id, starts_at) where status = 'booked';
create table lab_results (
  id integer primary key,
  patient_id integer not null references users(id),
  uploaded_by integer not null references users(id),
  filename text not null,
  stored_as text not null,
  type text not null,
  size integer not null,
  note text not null default '',
  created_at text not null
);
create table invoices (
  id integer primary key,
  appointment_id integer not null unique references appointments(id),
  currency text not null,
  subtotal decimal_text not null,
  tax decimal_text not null,
  total decimal_text not null,
  issued_at text not null
);
create table invoice_lines (
  id integer primary key,
  invoice_id integer not null references invoices(id),
  label text not null,
  qty integer not null,
  unit decimal_text not null,
  amount decimal_text not null
);
create table outbox (
  id integer primary key,
  appointment_id integer not null references appointments(id),
  kind text not null,
  locale text not null,
  body text not null,
  created_at text not null,
  unique (appointment_id, kind)
);
