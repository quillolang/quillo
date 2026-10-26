// Drives tests/browser/media.qwe: how soon the first frame shows, and what
// was decoded in the next seconds.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const el = document.querySelector("quillo-media");
  const out = {};
  for (let i = 0; i < 400; i++) {
    if (el.mediaStats && el.mediaStats.video > 0 && out.firstFrameMs === undefined) out.firstFrameMs = Math.round(performance.now());
    if (el.mediaStats && el.mediaStats.video >= 45 && el.mediaStats.audio > 0) break;
    await t(25);
  }
  // Frames keep coming, as they are sent.
  const before = el.mediaStats ? el.mediaStats.video : 0;
  await t(1000);
  const s = el.mediaStats || {};
  out.liveFrames = (s.video || 0) - before;
  const c = el.querySelector("canvas");
  const seen = new Set();
  if (c && c.width) {
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    for (let i = 0; i < d.length; i += 4 * 97) seen.add((d[i] >> 5) + "," + (d[i + 1] >> 5) + "," + (d[i + 2] >> 5));
  }
  Object.assign(out, {
    state: el.dataset.state, video: s.video, audio: s.audio, codec: s.codec,
    width: c && c.width, colors: seen.size, via: window.__quilloChannelVia, errors: window.__quilloErrors || [],
  });
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
