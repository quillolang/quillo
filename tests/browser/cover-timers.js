// Drives tests/browser/cover-timers.qwe: timers that fail, a component gone
// mid-timer, @mount failing, and `listen` reopening and stopping.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = f();
      if (v) return v;
      await tick(20);
    }
    return null;
  };
  const text = (s) => ($(s) ? $(s).textContent : null);
  const hits = async () => (await fetch("/hits")).json();
  const errorsSince = (n) => window.__quilloErrors.slice(n);
  try {
    await until(() => window.__quillo_booted);

    // `run_after` whose function fails: said, and the next timer still runs
    // (a negative delay is no delay).
    let n = window.__quilloErrors.length;
    $("#run-after").click();
    out.after = await until(() => text("#after") !== "none" && text("#after"));
    await until(() => window.__quilloErrors.length > n);
    out.runAfterError = errorsSince(n);

    // `every` whose function fails once goes on ticking, until it is stopped.
    n = window.__quilloErrors.length;
    $("#every").click();
    await until(() => text("#ticks") === "5");
    await tick(200);
    out.ticks = text("#ticks");
    out.everyError = errorsSince(n);

    // A component gone before its `run_after` fires: the timer goes with it,
    // and so does its `every`.
    $("#show").click();
    await until(() => text("#pings").split(",").filter((x) => x === "tick").length >= 2);
    $("#hide").click();
    await until(() => !$("#timed"));
    const atHide = text("#pings");
    await tick(450);
    out.pingsAfterHide = text("#pings") === atHide;
    out.afterNeverRan = !text("#pings").includes("after");
    // Shown again and left on: both timers run.
    $("#show").click();
    await until(() => text("#pings").includes("after"), 3000);
    $("#hide").click();
    out.afterWhenKept = text("#pings").includes("after");

    // An @mount that fails is said; the one beside it still runs.
    n = window.__quilloErrors.length;
    $("#mount").click();
    out.mounted = await until(() => text("#mounted") !== "none" && text("#mounted"));
    await until(() => window.__quilloErrors.length > n);
    out.mountError = errorsSince(n);

    // `listen`: refused (503), opened again a second later, and heard.
    n = window.__quilloErrors.length;
    const t1 = Date.now();
    $("#streams").click();
    out.flaky = await until(() => text("#flaky") !== null && text("#flaky") !== "waiting" && text("#flaky"), 10000);
    out.flakyWaited = Date.now() - t1 >= 900;
    // A function that fails stops the listening: no more values, not opened again.
    await until(() => text("#bad") === "1,2");
    await tick(700);
    out.bad = text("#bad");
    out.hits = await hits();
    out.listenError = errorsSince(n);
    out.errors = window.__quilloErrors.filter((e) => e !== "division by zero");
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
