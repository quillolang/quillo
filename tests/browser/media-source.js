// Drives tests/browser/media-publish.qwe at /page in a Chrome that lets no
// page have the camera: a sender is refused it, and a MediaStream given as
// its `source` goes instead (before it starts, after it was refused, and in
// place of another); a source the encoders cannot take, then one they can.
// With `?large`, it sends a 1920x1080 source and a 4-channel one, for what
// `a_large_source_and_one_of_four_channels_are_sent` expects.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end && !f(); ) await t(25); return f(); };
  const make = (attrs) => {
    const el = document.createElement("quillo-media");
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
    return el;
  };
  // A canvas of one colour, painted often (a canvas's stream sends frames as it changes).
  const paints = [];
  const canvas = (w, h, color) => {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const g = c.getContext("2d");
    let n = 0;
    paints.push(setInterval(() => { g.fillStyle = color; g.fillRect(0, 0, w, h); g.fillStyle = n++ % 2 ? "#000" : "#fff"; g.fillRect(0, 0, 2, 2); }, 30));
    return c.captureStream(30);
  };
  // What the middle of a player shows, roughly: "r,g,b" in eighths.
  const shows = (el) => {
    const c = el.querySelector("canvas");
    if (!c || !c.width) return null;
    const d = c.getContext("2d").getImageData(c.width >> 1, c.height >> 1, 1, 1).data;
    return [d[0] >> 5, d[1] >> 5, d[2] >> 5].join(",");
  };
  const RED = "7,0,0", BLUE = "0,0,7", GREEN = "0,7,0";
  const out = {};
  const ask = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  let asked = 0;
  navigator.mediaDevices.getUserMedia = (want) => { asked++; return ask(want); };

  if (location.search === "?large") {
    // A screen's size, and sound of four channels.
    const big = make({ publish: "edge:large" });
    big.source = canvas(1920, 1080, "#f00");
    const ctx = new AudioContext();
    const dest = ctx.createMediaStreamDestination();
    dest.channelCount = 4;
    const o = ctx.createOscillator();
    o.connect(dest);
    o.start();
    const wide = make({ publish: "edge:wide" });
    wide.source = new MediaStream([canvas(320, 240, "#00f").getVideoTracks()[0], dest.stream.getAudioTracks()[0]]);
    document.body.append(big, wide);
    await until(() => big.mediaStats.video > 10 && wide.mediaStats.audio > 10, 5000);
    out.large = { state: big.dataset.state, video: big.mediaStats.video };
    out.wide = { state: wide.dataset.state, video: wide.mediaStats.video > 0, audio: wide.mediaStats.audio };
    out.errors = window.__quilloErrors || [];
    document.body.setAttribute("data-result", JSON.stringify(out));
    return;
  }

  // Refused the camera; then a source given goes.
  const denied = make({ publish: "edge:denied" });
  const deniedPlay = make({ stream: "edge:denied", muted: "" });
  document.body.append(denied, deniedPlay);
  await until(() => denied.dataset.state === "refused", 5000);
  out.denied = { state: denied.dataset.state, asked, source: denied.source };
  denied.source = canvas(320, 240, "#f00");
  out.denied.then = denied.dataset.state;
  await until(() => shows(deniedPlay) === RED, 10000);
  out.denied.shows = shows(deniedPlay);
  out.denied.player = deniedPlay.dataset.state;

  // A source given before it starts: the camera is not asked.
  asked = 0;
  const given = make({ publish: "edge:given" });
  const blue = canvas(320, 240, "#00f");
  given.source = blue;
  const givenPlay = make({ stream: "edge:given", muted: "" });
  document.body.append(given, givenPlay);
  await until(() => shows(givenPlay) === BLUE, 10000);
  out.given = { state: given.dataset.state, asked, source: given.source === blue, shows: shows(givenPlay), tracks: given.querySelector("video").srcObject.getTracks().length };
  // Another in its place: the player shows the new one.
  const green = canvas(640, 360, "#0f0");
  given.source = green;
  await until(() => shows(givenPlay) === GREEN, 10000);
  out.given.then = { state: given.dataset.state, shows: shows(givenPlay), width: givenPlay.querySelector("canvas").width };
  // None: nothing more is sent.
  given.source = null;
  const n = given.mediaStats.sent;
  await t(600);
  out.given.none = { state: given.dataset.state, sent: given.mediaStats.sent - n, source: given.source };

  // A source too small to encode (H.264 takes 16 pixels or more), then one
  // that is not: it goes.
  const tiny = make({ publish: "edge:tiny" });
  tiny.source = canvas(8, 8, "#f00");
  const tinyPlay = make({ stream: "edge:tiny", muted: "" });
  document.body.append(tiny, tinyPlay);
  await t(1000);
  out.tiny = { video: tiny.mediaStats.video };
  tiny.source = canvas(320, 240, "#f00");
  await until(() => shows(tinyPlay) === RED, 10000);
  out.tiny.then = { state: tiny.dataset.state, shows: shows(tinyPlay) };
  for (const el of [denied, deniedPlay, given, givenPlay, tiny, tinyPlay]) el.remove();
  for (const p of paints) clearInterval(p);
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
