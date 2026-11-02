(async () => {
  const out = {};
  const tick = (ms = 80) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  const type = async (el, v) => { el.focus(); await tick(300); el.value = v; el.dispatchEvent(new Event("input", { bubbles: true })); await tick(); };
  try {
    for (let i = 0; i < 200 && !$("#name"); i++) await tick(30);
    // What a person does.
    await type($("#name"), "Ana");
    $("#agree").click(); await tick();
    $("#half").click(); await tick();
    $("#size").value = "s"; $("#size").dispatchEvent(new Event("input", { bubbles: true })); await tick();
    await type($("#code"), "abc");
    await type($("#fixed"), "typed");
    await type($("#locked"), "changed");
    await type($("#qty"), "3");
    $("#inc").click(); await tick();
    $("#inc").click(); await tick();
    $("#more").open = true; await tick(200);
    $("#ask").close(); await tick(200);
    const p = $("#player");
    for (let i = 0; i < 100 && p.readyState < 1; i++) await tick(30);
    p.muted = true; await tick();
    p.volume = 0.25; await tick();
    p.playbackRate = 2; await tick();
    p.currentTime = 0.5; await tick(300);
    out.person = $("#out").textContent;
    out.inputs = [$("#name").value, $("#code").value, $("#locked").value, $("#fixed").value, $("#half").indeterminate, $("#qty").value];
    // What the program does: the element follows.
    $("#set").click(); await tick(200);
    out.program = $("#out").textContent;
    out.player = [p.volume, p.playbackRate, p.muted];
    // Out of range from the program: brought into the player's, nothing thrown.
    $("#wild").click(); await tick(200);
    out.wild = [p.volume, p.playbackRate];
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
