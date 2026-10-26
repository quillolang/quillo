-- Two-step sign-in: the secret of each person's authenticator app (empty
-- when they have none), the time step of the code used last (a code works
-- once), and the hashes of their recovery codes not used yet.
alter table users add column totp_secret text not null default '';
alter table users add column totp_step integer not null default 0;
alter table users add column recovery_hashes text not null default '[]';
