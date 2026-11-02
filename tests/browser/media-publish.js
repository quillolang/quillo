// Drives tests/browser/media-publish.qwe at /: the page's (fake) camera and
// microphone go to stream cam, and the page plays cam. Then the sound is
// turned off and on, the sender leaves and comes back, and the player goes.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end && !f(); ) await t(25); return f(); };
  const [p, v] = document.querySelectorAll("quillo-media");
  const out = {};
  // Sending, and playing both tracks.
  await until(() => v.mediaStats && v.mediaStats.video > 30 && v.mediaStats.audio > 20 && p.mediaStats.sent > 50, 20000);
  const ps = p.mediaStats, vs = v.mediaStats, preview = p.querySelector("video");
  const c = v.querySelector("canvas");
  out.sending = {
    state: p.dataset.state, sent: ps.sent > 50, bytes: ps.bytes > 10000, video: ps.video > 0, audio: ps.audio > 0,
    previewTracks: preview.srcObject && preview.srcObject.getTracks().map((x) => x.kind).sort().join(","),
    source: p.source,
  };
  out.playing = {
    state: v.dataset.state, codec: vs.codec, width: c.width, height: c.height,
    // The sound plays a little ahead, from a running AudioContext.
    sound: vs.actx && vs.actx.state, ahead: vs.at > vs.actx.currentTime && vs.at < vs.actx.currentTime + 0.6,
  };
  // The button turns the sound off: no more audio decoded.
  const b = v.querySelector("button");
  out.button = [b.textContent];
  b.click();
  out.button.push(b.textContent);
  await t(300);
  const a0 = vs.audio;
  await t(700);
  out.mutedAudio = vs.audio - a0;
  b.click();
  out.button.push(b.textContent);
  const a1 = vs.audio;
  await until(() => vs.audio > a1 + 10, 5000);
  out.unmutedAudio = vs.audio - a1 > 10;
  // A click on the player itself is a gesture that may start the sound.
  v.click();
  // The sender leaves: its camera is let go, and nothing more is sent.
  const tracks = preview.srcObject.getTracks();
  const player = p.player;
  p.remove();
  player.stop(); // twice: nothing more happens
  const sent = ps.sent;
  await t(600);
  out.left = { player: p.player, tracks: tracks.map((x) => x.readyState).join(","), sentAfter: ps.sent - sent };
  // With no frames for 3 seconds, the player waits.
  out.left.waiting = await until(() => v.dataset.state === "waiting", 6000);
  // It comes back: sending again, and the player plays again.
  const before = vs.video;
  document.body.prepend(p);
  out.back = { stats: p.mediaStats !== ps, first: p.dataset.state };
  await until(() => p.dataset.state === "sending" && p.mediaStats.sent > 20 && vs.video > before + 20 && v.dataset.state === "playing", 15000);
  out.back.state = p.dataset.state;
  out.back.sent = p.mediaStats.sent > 20;
  out.back.played = vs.video > before + 20;
  out.back.player = v.dataset.state;
  // The player goes: its decoders and its AudioContext are closed.
  v.remove();
  await t(300);
  const n = vs.video;
  await t(500);
  out.gone = { video: vs.vd ? vs.vd.state : null, audio: vs.ad ? vs.ad.state : null, sound: vs.actx.state, decoded: vs.video - n };
  out.via = (window[Symbol.for("quillo")] || {}).via;
  out.errors = window.__quilloErrors || [];
  p.remove();
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
