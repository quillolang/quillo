# wtransport 0.7.2, patched for Quillo

This folder is the published source of [wtransport](https://crates.io/crates/wtransport) 0.7.2
(`wtransport-0.7.2.crate` from crates.io, git d022a6a), used by Quillo through `[patch.crates-io]`
in the root Cargo.toml, with one fix (FEEDBACK75.md, Q-1124; branch feedback86-89). wtransport is
MIT OR Apache-2.0, as its Cargo.toml says; this changed copy is too, and this file states the
changes as Apache-2.0 §4(b) asks.

**Drop this folder and the `[patch.crates-io]` entry once a wtransport release fixes the bug.**
Test it with tests/f812_wt.rs, which fails without the fix.

## The bug

wtransport's connection driver (`src/driver/mod.rs`, `Worker::run_impl`) waits in one `select!`
on accepting streams and datagrams and on `run_control_streams`, which reads the peer's HTTP/3
control stream (`RemoteSettingsStream::run`) and the session's CONNECT stream
(`ConnectStream::run`). Each turn of the loop builds a new `run_control_streams` future and drops
the old one whenever another branch is ready first. Those reads go through
`Frame::read_async`, which is not safe to cancel: the bytes it has already taken from the stream
(the frame's type, its length, part of its payload) are lost with the dropped future.

So a peer whose SETTINGS frame arrives in two packets, with a new stream or a datagram arriving
between them, has the frame read again from its middle. Depending on where the frame was cut, the
connection closes with H3_FRAME_UNEXPECTED (0x105), H3_EXCESSIVE_LOAD (0x107) or
H3_MISSING_SETTINGS (0x10a), or the session never starts. The same happens to a capsule on the
CONNECT stream: a CLOSE_WEBTRANSPORT_SESSION split across packets is never seen, or kills the
connection. It happens on lossy networks with real browsers, and a peer can do it on purpose.
Quillo's CI hit it once in 73 runs, and the tests retried around it (commit 47869ad) until this
patch.

Upstream knows it: wtransport issue #331 (open since 2026-09-29) and pull request #334 (open),
which fixes it among other things.

## The fix

In `src/driver/streams/settings.rs` (`RemoteSettingsStream`) and
`src/driver/streams/connect.rs` (`ConnectStream`), each changed line marked
`// Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89)`:

- A new field, `reading`, holds the read of one frame as a boxed future that owns the stream
  while the frame is incomplete and hands it back with the frame.
- `read_frame` starts that read when none is under way, and otherwise resumes it. When the
  `select!` drops `run`, the read stays in `reading` and the next `run` picks it up where it was.
  Nothing is read twice or lost.
- `is_empty` also counts a stream held by `reading`, so the checks for a second control stream
  and a second session stay as they were.

The `select!` and the rest of the driver are unchanged. One allocation per control frame
(SETTINGS once a connection; a capsule now and then). Datagrams and WebTransport streams do not
pass through this code.

Left as they were:

- The QPACK encoder and decoder streams. Their reads (`read_exact` into a scratch buffer) can lose
  bytes the same way, but wtransport throws those bytes away anyway.
- `LocalSettingsStream::run`, which only waits for the stream to be stopped and takes nothing
  from it.

To see the whole change:

    mkdir -p /tmp/w && tar -xzf ~/.cargo/registry/cache/index.crates.io-*/wtransport-0.7.2.crate -C /tmp/w
    diff -ru /tmp/w/wtransport-0.7.2 vendor/wtransport-0.7.2
