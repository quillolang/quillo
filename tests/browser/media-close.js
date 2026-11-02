// Drives tests/browser/media-publish.qwe at /page, with a (fake) camera:
// the program ends a stream a page sends (`media.close`), refuses a sender
// (`publish` says no), and has no `media.on` for another (which it should
// refuse: see `a_stream_no_media_on_names_is_refused`); two senders of one
// stream in a page, one of which goes.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end && !f(); ) await t(25); return f(); };
  const make = (attrs) => {
    const el = document.createElement("quillo-media");
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
    return el;
  };
  const out = {};
  // `media.close`: the sender and the player are told it ended.
  const send = make({ publish: "ending", "no-audio": "" });
  const play = make({ stream: "ending", muted: "" });
  const refused = make({ publish: "closed", "no-audio": "" });
  const unknown = make({ publish: "nobody", "no-audio": "" });
  document.body.append(send, play, refused, unknown);
  await until(() => send.mediaStats.sent > 20 && play.mediaStats.video > 10 && refused.dataset.state === "refused", 15000);
  out.before = { send: send.dataset.state, play: play.dataset.state };
  out.refused = { state: refused.dataset.state, sent: refused.mediaStats.sent, encoded: refused.mediaStats.video > 0 };
  out.unknown = { state: unknown.dataset.state, sent: unknown.mediaStats.sent };
  out.closed = await (await fetch("/close", { method: "POST" })).text();
  await until(() => send.dataset.state === "ended" && play.dataset.state === "ended", 5000);
  const sent = send.mediaStats.sent, shown = play.mediaStats.video;
  await t(1500);
  // It stays ended: nothing more is sent or played.
  out.after = { send: send.dataset.state, play: play.dataset.state, sent: send.mediaStats.sent - sent, played: play.mediaStats.video - shown, decoder: play.mediaStats.vd };
  out.again = await (await fetch("/close", { method: "POST" })).text();
  for (const el of [send, play, refused, unknown]) el.remove();

  // Two senders of one stream: one going leaves the other sending.
  const one = make({ publish: "edge:twice", "no-audio": "" });
  const two = make({ publish: "edge:twice", "no-audio": "" });
  document.body.append(one, two);
  await until(() => one.mediaStats.sent > 10 && two.mediaStats.sent > 10, 15000);
  one.remove();
  const n = two.mediaStats.sent;
  await t(800);
  out.twice = { state: two.dataset.state, sent: two.mediaStats.sent - n > 5 };
  two.remove();
  out.streams = await (await fetch("/streams")).text();
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
