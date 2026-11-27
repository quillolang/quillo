# tokio-postgres 0.7.18, patched for Quillo

Byte-identical to the published crate but `src/query.rs` and `src/client.rs`, which add
`Client::query_after(prefix, statement, params)`: the prefix's statements (each prepared, with no
parameters, so bound and run without a parse) and then the statement, sent in one request under ONE Sync. PostgreSQL runs them in
one implicit transaction, so where a prefix statement fails, the statement is skipped.

Why: on a database whose own code uses a session's state, Quillo sets each shared connection's
session back in front of each statement (FEEDBACK75.md, Q-1060). Sent as its own query, a reset
that failed was rolled back as a whole and the statement after it ran on under the session left;
`begin; reset` ... `commit` around it cost a third request each time (+8.7%). Under one Sync the
reset and the statement are one request and one transaction.

Drop it when tokio-postgres offers a way to send several statements under one Sync.
