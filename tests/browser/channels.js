// Drives tests/browser/channels.qwe: this page and a second one (in a
// frame) on the same channel.
(async () => {
  const out = {};
  const t = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = f();
      if (v) return v;
      await t(30);
    }
    return null;
  };
  const items = (doc) => [...doc.querySelectorAll("li")].map((l) => l.textContent);
  try {
    out.via = await until(() => window.__quilloChannelVia);
    const fr = document.createElement("iframe");
    fr.src = "/";
    document.body.appendChild(fr);
    out.viaFrame = await until(() => fr.contentWindow && fr.contentWindow.__quilloChannelVia);
    const other = () => fr.contentDocument;
    // Both joined: the server counts them.
    for (let i = 0; i < 100; i++) {
      out.peersBefore = Number(await (await fetch("/peers")).text());
      if (out.peersBefore === 2) break;
      await t(50);
    }
    document.querySelector("#send").click();
    out.bothGot = !!(await until(() => items(document).includes("hi") && items(other()).includes("hi")));
    // Larger than a datagram: it goes on the stream.
    other().querySelector("#big").click();
    out.bigGot = !!(await until(() => items(document).some((x) => x.length === 5000)));
    await fetch("/bcast/server-says");
    out.serverGot = !!(await until(() => items(document).includes("server-says") && items(other()).includes("server-says")));
    out.udpGot = !!(await until(() => items(document).some((x) => x.startsWith("udp reading")), 10000));
    // Reliable messages arrive, and in order.
    document.querySelector("#order").click();
    const want = Array.from({ length: 20 }, (_, i) => "o" + (i + 1));
    out.orderKept = !!(await until(() => {
      const got = items(other()).filter((x) => /^o\d+$/.test(x));
      return got.length === 20 && got.every((x, i) => x === want[i]);
    }));
    document.querySelector("#who").click();
    out.who = (await until(() => items(document).find((x) => x.startsWith("who:"))) || "").slice(4);
    out.vip = await (await fetch("/vip")).text();
    out.vipGotIn = items(document).includes("vip got in");
    // A page that goes away leaves its channels.
    fr.remove();
    for (let i = 0; i < 200; i++) {
      out.peersAfter = Number(await (await fetch("/peers")).text());
      if (out.peersAfter === 1) break;
      await t(50);
    }
    out.errors = window.__quilloErrors || [];
  } catch (e) {
    out.exception = String(e && e.stack || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
