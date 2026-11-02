// Drives tests/browser/media-publish.qwe at /, with a (fake) camera: once
// the page sends and plays, it asks for /restart-me, and the test stops the
// server and starts it again 4 s later. The page connects again, joins
// again, and goes on sending and playing.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end && !f(); ) await t(25); return f(); };
  const [p, v] = document.querySelectorAll("quillo-media");
  const out = {};
  out.before = await until(() => p.mediaStats.sent > 30 && v.dataset.state === "playing", 20000);
  await fetch("/restart-me").catch(() => {});
  // The server is down for 4 s: the player has no frames, and waits.
  out.gap = await until(() => v.dataset.state === "waiting", 10000);
  const sent = p.mediaStats.sent, shown = v.mediaStats.video;
  out.after = await until(() => p.mediaStats.sent > sent + 30 && v.mediaStats.video > shown + 30 && v.dataset.state === "playing", 30000);
  out.state = p.dataset.state;
  out.via = window.__quilloChannelVia;
  out.errors = window.__quilloErrors || [];
  p.remove();
  v.remove();
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
