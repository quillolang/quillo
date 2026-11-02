// Drives tests/browser/media-publish.qwe at /page, with a (fake) camera and
// microphone: `no-video` and `no-audio`, a sender that goes before or while
// the camera is asked, a source given while it is asked, and browsers
// without track processors or without WebCodecs.
(async () => {
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms) => { for (const end = Date.now() + ms; Date.now() < end && !f(); ) await t(25); return f(); };
  const make = (attrs) => {
    const el = document.createElement("quillo-media");
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
    return el;
  };
  const tracks = (el) => { const s = el.querySelector("video") && el.querySelector("video").srcObject; return s ? s.getTracks().map((x) => x.kind).sort().join(",") : null; };
  const out = {};
  // The camera alone, the microphone alone, and neither.
  const vOnly = make({ publish: "edge:video", "no-audio": "" });
  const aOnly = make({ publish: "edge:audio", "no-video": "" });
  const none = make({ publish: "edge:none", "no-video": "", "no-audio": "" });
  document.body.append(vOnly, aOnly, none);
  await until(() => vOnly.mediaStats.sent > 20 && aOnly.mediaStats.sent > 20 && none.dataset.state === "refused", 15000);
  out.videoOnly = { state: vOnly.dataset.state, tracks: tracks(vOnly), video: vOnly.mediaStats.video > 20, audio: vOnly.mediaStats.audio };
  out.audioOnly = { state: aOnly.dataset.state, tracks: tracks(aOnly), video: aOnly.mediaStats.video, audio: aOnly.mediaStats.audio > 20 };
  out.neither = { state: none.dataset.state, sent: none.mediaStats.sent };
  for (const el of [vOnly, aOnly, none]) el.remove();

  // The camera, asked slowly, and what it gave.
  const ask = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  const asked = [];
  navigator.mediaDevices.getUserMedia = async (want) => {
    const s = await ask(want);
    asked.push(s);
    await t(500);
    return s;
  };
  // Gone before the camera is asked: it never is.
  const early = make({ publish: "edge:early" });
  document.body.append(early);
  early.remove();
  await t(300);
  out.early = { asked: asked.length, state: early.dataset.state };
  // Gone while it is asked: what the camera gave is let go.
  const during = make({ publish: "edge:during" });
  document.body.append(during);
  await until(() => asked.length === 1, 5000);
  during.remove();
  await t(800);
  out.during = { asked: asked.length, tracks: asked[0].getTracks().map((x) => x.readyState).join(","), state: during.dataset.state };
  // A source given while the camera is asked goes instead, and the camera is let go.
  const c = document.createElement("canvas");
  c.width = 320;
  c.height = 240;
  const g = c.getContext("2d");
  let hue = 0;
  const paint = setInterval(() => { g.fillStyle = `hsl(${hue++ % 360},80%,50%)`; g.fillRect(0, 0, 320, 240); }, 30);
  const canvasStream = c.captureStream(30);
  const given = make({ publish: "edge:given" });
  document.body.append(given);
  await until(() => asked.length === 2, 5000);
  given.source = canvasStream;
  out.given = { state: given.dataset.state, source: given.source === canvasStream, preview: given.querySelector("video").srcObject === canvasStream };
  await t(800);
  out.given.camera = asked[1].getTracks().map((x) => x.readyState).join(",");
  out.given.stillPreview = given.querySelector("video").srcObject === canvasStream;
  await until(() => given.mediaStats.sent > 10, 5000);
  out.given.sent = given.mediaStats.sent > 10;
  given.remove();
  navigator.mediaDevices.getUserMedia = ask;

  // Without track processors: the preview drawn on a canvas, the picture alone.
  const processor = window.MediaStreamTrackProcessor;
  delete window.MediaStreamTrackProcessor;
  const drawn = make({ publish: "edge:drawn" });
  const watch = make({ stream: "edge:drawn", muted: "" });
  document.body.append(drawn, watch);
  await until(() => drawn.mediaStats.video > 30 && watch.mediaStats.video > 20, 15000);
  out.drawn = { state: drawn.dataset.state, video: drawn.mediaStats.video > 30, audio: drawn.mediaStats.audio, played: watch.mediaStats.video > 20, player: watch.dataset.state, width: watch.querySelector("canvas").width };
  drawn.remove();
  watch.remove();
  window.MediaStreamTrackProcessor = processor;

  // Without WebCodecs: the element says so.
  const [enc, dec] = [window.VideoEncoder, window.VideoDecoder];
  delete window.VideoEncoder;
  delete window.VideoDecoder;
  const cantSend = make({ publish: "edge:old" });
  const cantPlay = make({ stream: "edge:old" });
  document.body.append(cantSend, cantPlay);
  out.unsupported = { send: [cantSend.dataset.state, cantSend.textContent], play: [cantPlay.dataset.state, cantPlay.textContent] };
  cantSend.remove();
  cantPlay.remove();
  window.VideoEncoder = enc;
  window.VideoDecoder = dec;
  clearInterval(paint);
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
