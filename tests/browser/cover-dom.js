// Drives tests/browser/cover-dom.qwe: events, request bodies, ranges, the
// dialog's keys, lists and the idempotency key, each read back from the page.
(async () => {
  const out = {};
  const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const until = async (f, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const v = f();
      if (v) return v;
      await tick(30);
    }
    return null;
  };
  const text = (s) => $(s).textContent;
  const ids = () => [...$("#rows").children].map((li) => li.id || li.textContent);
  try {
    await until(() => window.__quillo_booted);

    // What a handler reads of its event.
    $("#tick").click();
    out.checked = JSON.parse(await until(() => text("#checked") !== "none" && text("#checked")));
    const dt = new DataTransfer();
    dt.items.add(new File(["\x89PNG"], "photo.png", { type: "image/png" }));
    $("#pic").files = dt.files;
    $("#submit").click();
    out.form = JSON.parse(await until(() => text("#sent") !== "none" && text("#sent")));
    out.stayed = location.pathname;
    $("#loose").click();
    await until(() => text("#fields") !== "none");
    out.looseForm = JSON.parse(text("#loose-form"));
    out.fields = JSON.parse(text("#fields"));

    // HTML as a request's body, with its type unless the call gave one.
    $("#post-raw").click();
    out.bodies = JSON.parse(await until(() => text("#bodies") !== "[]" && text("#bodies")));

    // `range` with a step, up, down and empty; a step of 0 is an error.
    $("#ranges").click();
    out.ranges = JSON.parse(await until(() => text("#range-out") !== "none" && text("#range-out")));
    const before = window.__quilloErrors.length;
    $("#range-zero").click();
    await until(() => window.__quilloErrors.length > before);
    out.zeroError = window.__quilloErrors.slice(before);
    out.zeroLeft = text("#range-out");

    // Tab in the dialog goes between its two buttons, and nowhere else.
    $("#ask").click();
    await until(() => $(".q-dialog"));
    await tick(50);
    const focus = () => document.activeElement && document.activeElement.className;
    const tab = (shift) => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: !!shift, bubbles: true, cancelable: true }));
    out.tabs = [focus()];
    tab();
    out.tabs.push(focus());
    tab(true);
    out.tabs.push(focus());
    // Focus somewhere else on the page: Tab brings it back to the dialog.
    $("#ask").focus();
    tab();
    out.tabs.push(focus());
    $(".q-dialog .q-cancel").focus();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    out.stillOpen = !!$(".q-dialog");
    $(".q-dialog .q-cancel").click();
    out.answer = await until(() => text("#answer") !== "none" && text("#answer"));

    // A list with elements beside its rows: emptied, its neighbours stay.
    const rows = () => [...document.querySelectorAll("#rows .row")];
    out.rowsStart = ids();
    $("#rows-clear").click();
    await until(() => rows().length === 0);
    out.rowsCleared = ids();
    $("#rows-abc").click();
    await until(() => rows().length === 3);
    out.rowsBack = ids();
    // Every key new: the old rows go at once.
    const oldA = rows()[0];
    $("#rows-new").click();
    await until(() => rows().length === 2);
    out.rowsNew = ids();
    out.oldGone = !oldA.isConnected;
    $("#rows-abc").click();
    await until(() => rows().length === 3);
    // A browser without moveBefore (Firefox and Safari before it came): rows
    // still move, as the same elements.
    const had = Object.getOwnPropertyDescriptor(Element.prototype, "moveBefore");
    delete Element.prototype.moveBefore;
    const els = rows();
    els.forEach((li) => { li.__mark = li.textContent; });
    $("#rows-reverse").click();
    await until(() => rows()[0].textContent === "c");
    out.rowsReversed = ids();
    out.sameElements = rows().map((li) => li.__mark);
    $("#rows-five").click();
    await until(() => rows().length === 5);
    out.rowsFive = ids();
    out.keptMarks = rows().map((li) => li.__mark || null);
    if (had) Object.defineProperty(Element.prototype, "moveBefore", had);

    // Where crypto.randomUUID is missing (a page over plain http from
    // another machine): a key from getRandomValues, the same for two clicks.
    Object.defineProperty(crypto, "randomUUID", { value: undefined, configurable: true });
    const keys = [];
    const fetched = window.fetch;
    window.fetch = function (u, init) {
      if (String(u).includes("/rpc/pay")) {
        const h = (init && init.headers) || {};
        keys.push(h instanceof Headers ? h.get("idempotency-key") : h["idempotency-key"]);
      }
      return fetched.apply(this, arguments);
    };
    $("#pay").click();
    $("#pay").click();
    await until(() => text("#said") === "click");
    await tick(100);
    $("#count").click();
    out.paidOnce = await until(() => text("#n") !== "0" && text("#n"));
    $("#pay").click();
    await until(() => keys.length === 2);
    await tick(400);
    $("#count").click();
    out.paidTwice = await until(() => text("#n") === "2" && text("#n"), 3000) || text("#n");
    window.fetch = fetched;
    out.keys = keys.map((k) => (/^[0-9a-f]{32}$/.test(k || "") ? "hex32" : k));
    out.keysDiffer = keys.length === 2 && keys[0] !== keys[1];
    out.errors = window.__quilloErrors.filter((e) => !out.zeroError.includes(e));
  } catch (e) {
    out.exception = String((e && e.stack) || e);
  }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
