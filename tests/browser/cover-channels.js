// Drives tests/browser/cover-channels.qwe: listeners on one channel, one
// failing, removed one by one and one brought back, and what the server
// counts and sends.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = await f();
      if (v) return v;
      await tick(30);
    }
    return null;
  };
  const get = async (p) => (await fetch(p)).text();
  const peers = (n) => until(async () => (await get("/peers")) === String(n));
  const items = (tag) => [...document.querySelectorAll(`.listener.${tag} li`)].map((l) => l.textContent);
  try {
    out.via = await until(() => window.__quilloChannelVia);
    out.joined = !!(await peers(1));
    // Sent from @mount, before the page was let in: kept, then sent once.
    out.early = await until(async () => {
      const log = JSON.parse(await get("/log"));
      return log.includes("early bird") && log.filter((x) => x === "early bird").length;
    });
    await get("/bcast/one");
    out.bothGotOne = !!(await until(() => items("a").includes("one") && items("b").includes("one")));
    // a's function fails on "boom": said, b still has it, and a hears on.
    await get("/bcast/boom");
    out.boomBoth = !!(await until(() => items("a").includes("boom") && items("b").includes("boom")));
    await get("/bcast/after-boom");
    out.aHearsOn = !!(await until(() => items("a").includes("after-boom")));
    out.boomErrors = window.__quilloErrors.slice();
    // One of the two removed: the page is still in the channel.
    $("#toggle-a").click();
    await until(() => !$(".listener.a"));
    await tick(300);
    out.peersWithB = await get("/peers");
    await get("/bcast/two");
    out.bGotTwo = !!(await until(() => items("b").includes("two")));
    // Both removed: the page leaves the channel.
    $("#toggle-b").click();
    out.left = !!(await peers(0));
    out.sentToNone = await get("/bcast/three");
    // One back: joined again, with its own new list.
    $("#toggle-a").click();
    out.rejoined = !!(await peers(1));
    await get("/bcast/four");
    out.aAgain = await until(() => items("a").includes("four") && items("a"));
    $(".listener.a .send").click();
    out.sentAfterRejoin = !!(await until(() => items("a").includes("a says")));
    out.errors = window.__quilloErrors.slice(out.boomErrors.length);
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
