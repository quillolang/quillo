// Scrolls tests/programs/scroll.qwe and records what is in the page.
(async () => {
  const out = {};
  const tick = (ms = 50) => new Promise((r) => setTimeout(r, ms));
  const waitFor = async (f, n = 200) => { for (let i = 0; i < n; i++) { if (f()) return true; await tick(); } return false; };
  const $ = (s) => document.querySelector(s);
  const rows = () => document.querySelectorAll(".row").length;
  const ids = () => [...document.querySelectorAll(".row .id")].map((e) => +e.textContent.slice(1));
  const loaded = () => +$(".loaded").textContent;
  try {
    await waitFor(() => window.quillo);
    await tick(200);
    const vp = $(".viewport");
    let most = 0;
    const scrollTo = async (px) => {
      vp.scrollTop = px;
      vp.dispatchEvent(new Event("scroll"));
      await tick(120);
      most = Math.max(most, rows());
    };
    out.start = { rows: rows(), loaded: loaded(), ids: [ids()[0], ids().at(-1)] };
    await scrollTo(100 * 36);
    out.atRow100 = { rows: rows(), ids: [ids()[0], ids().at(-1)], stats: $(".stats").textContent.replace(/\s+/g, " ").trim() };
    await scrollTo(190 * 36);
    out.loadedMore = await waitFor(() => loaded() > 200);
    out.afterLoad = { loaded: loaded(), rows: rows() };
    for (let k = 0; k < 12; k++) {
      await scrollTo(vp.scrollHeight);
      await waitFor(() => !$(".loading"));
    }
    out.deep = { loaded: loaded(), rows: rows(), lastId: ids().at(-1), spacer: $(".spacer").style.height };

    // The next page load fails (a dropped connection): the feed must not
    // freeze, and the next scroll loads again.
    const realFetch = window.fetch;
    let failNext = true;
    window.fetch = (u, o) => {
      if (failNext && String(u).includes("/_quillo/rpc/load_after")) {
        failNext = false;
        return Promise.reject(new TypeError("network down"));
      }
      return realFetch(u, o);
    };
    const before = loaded();
    await scrollTo(vp.scrollHeight);
    await tick(300);
    out.afterFailure = { failed: !failNext, stuckLoading: !!$(".loading") };
    await scrollTo(vp.scrollHeight + 1);
    out.recovered = await waitFor(() => loaded() > before);
    out.mostRowsAtOnce = most;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
