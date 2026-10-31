// Pages' worth of live data, for the repros of FEEDBACK24.md: real WebSockets, as many as
// wanted, with no browser. `node live-pages.mjs <port> <what>`.
import net from "node:net";

const [port, what] = process.argv.slice(2);
const P = Number(port);
const url = (p) => `http://127.0.0.1:${P}${p}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const live = (f) => fetch(url(`/_quillo/rpc/${f}`), { method: "POST", headers: { "content-type": "application/json", "x-quillo-live": "1", "sec-fetch-site": "same-origin" }, body: "[]" }).then((r) => r.json());

function open(since, topic, onFrame) {
  return new Promise((done) => {
    const s = net.connect(P, "127.0.0.1");
    s.write(`GET /_quillo/live?since=${since}&s=&t=${encodeURIComponent(topic)} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n`);
    let up = false;
    s.on("data", (d) => {
      if (!up) {
        up = d.toString().startsWith("HTTP/1.1 101");
        done(up ? s : null);
        return;
      }
      if ((d[0] & 15) === 1) onFrame();
    });
    s.on("error", () => done(null));
  });
}

const first = await live("rows");
if (what === "burst") {
  let frames = 0;
  await open(first.seq, first.topics.join(","), () => frames++);
  for (let i = 0; i < 30; i++) await fetch(url("/write"));
  await sleep(400);
  console.log(frames <= 8 ? "few frames for 30 writes" : "a frame for each write");
} else if (what === "together") {
  await Promise.all(Array.from({ length: 12 }, () => live("slow")));
} else if (what === "wakes") {
  await Promise.all(Array.from({ length: 50 }, () => open(first.seq, first.topics.join(","), () => {})));
  for (let i = 0; i < 20; i++) await fetch(url("/other"));
  const m = (await (await fetch(url("/_quillo/metrics"))).text()).match(/quillo_live_wakes_total (\d+)/);
  console.log(m ? `pages woken by what they do not follow: ${m[1]}` : "no count of pages woken");
} else if (what === "left") {
  const pages = await Promise.all(Array.from({ length: 256 }, () => open(first.seq, first.topics.join(","), () => {})));
  for (const s of pages) s.write(Buffer.from([0x88, 0x80, 1, 2, 3, 4]));
  await sleep(6500);
  const again = await Promise.all(Array.from({ length: 256 }, () => open(first.seq, first.topics.join(","), () => {})));
  console.log(again.every(Boolean) ? "places given back" : `places still held: ${again.filter((x) => !x).length}`);
}
process.exit(0);
