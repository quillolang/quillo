// Drives tests/browser/media-publish.qwe at /page, whose player plays stream
// inj. A worker sends inj as a page would (its own channel connection, a
// WebSocket), frames encoded here: a frame said to be a keyframe that is not
// one, then more frames at once than a player keeps up with (the page busy
// while they come), each time followed by a real keyframe.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end && !f(); ) await t(25); return f(); };
  const v = document.querySelector("quillo-media");
  const out = {};
  // A keyframe and two others, 64x64.
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d");
  const chunks = [];
  const enc = new VideoEncoder({ output: (ch) => { const d = new Uint8Array(ch.byteLength); ch.copyTo(d); chunks.push({ key: ch.type === "key", d }); }, error: () => {} });
  enc.configure({ codec: "avc1.42E01F", width: 64, height: 64, avc: { format: "annexb" }, latencyMode: "realtime" });
  for (let i = 0; i < 3; i++) {
    g.fillStyle = ["#f00", "#0f0", "#00f"][i];
    g.fillRect(0, 0, 64, 64);
    const f = new VideoFrame(c, { timestamp: i * 33333 });
    enc.encode(f, { keyFrame: i === 0 });
    f.close();
  }
  await enc.flush();
  out.encoded = chunks.map((x) => x.key);
  const ticket = (await (await fetch("/_quillo/wt-ticket", { method: "POST" })).json()).ticket;
  const worker = new Worker(URL.createObjectURL(new Blob([`
    let s, ts = 0;
    const name = new TextEncoder().encode("inj");
    const wire = (key, d) => {
      const b = new Uint8Array(7 + name.length + d.length);
      b[0] = name.length;
      b.set(name, 1);
      b[1 + name.length] = 1;
      b[2 + name.length] = key ? 1 : 0;
      new DataView(b.buffer).setUint32(3 + name.length, (ts += 3000));
      b.set(d, 7 + name.length);
      return b;
    };
    onmessage = async ({ data: m }) => {
      if (m.url) {
        s = new WebSocket(m.url);
        s.binaryType = "arraybuffer";
        await new Promise((ok) => (s.onopen = ok));
        s.send(JSON.stringify(["j", "media:inj"]));
        await new Promise((ok) => (s.onmessage = (e) => { if (typeof e.data === "string" && JSON.parse(e.data)[2] === true) ok(); }));
      } else {
        for (const [key, d] of m.frames) s.send(wire(key, d));
      }
      postMessage("done");
    };
  `])));
  const tell = (m) => new Promise((ok) => { worker.onmessage = () => ok(); worker.postMessage(m); });
  await tell({ url: location.origin.replace(/^http/, "ws") + "/_quillo/wt?t=" + encodeURIComponent(ticket) });
  await until(() => (window[Symbol.for("quillo")] || {}).via, 5000);
  await t(500);
  const [key, d1, d2] = chunks.map((x) => x.d);
  const playing = (n) => until(() => v.dataset.state === "playing" && v.mediaStats.video >= n, 5000);

  // Said to be a keyframe, and not one: dropped, and the next keyframe plays.
  await tell({ frames: [[true, d1]] });
  await t(500);
  out.notKey = { state: v.dataset.state, decoder: v.mediaStats.vd, video: v.mediaStats.video, keyframes: v.mediaStats.keyframes };
  await tell({ frames: [[true, key], [false, d1], [false, d2]] });
  out.notKey.then = await playing(3);

  // 700 frames at once, while the page is busy: more than the player keeps
  // up with, so it starts again at the next keyframe.
  const shown = v.mediaStats.video;
  const burst = [[true, key]];
  for (let i = 0; i < 700; i++) burst.push([false, i % 2 ? d2 : d1]);
  const sent = tell({ frames: burst });
  for (const end = performance.now() + 2500; performance.now() < end; ) {}
  await sent;
  await t(1000);
  out.behind = { state: v.dataset.state, decoder: v.mediaStats.vd, shown: v.mediaStats.video - shown };
  // Frames until the next keyframe are skipped.
  await tell({ frames: [[false, d1], [false, d2]] });
  await t(300);
  out.behind.skipped = v.mediaStats.video - shown === out.behind.shown && v.dataset.state === "waiting";
  await tell({ frames: [[true, key], [false, d1], [false, d2]] });
  out.behind.then = await playing(shown + 3);
  worker.terminate();
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
