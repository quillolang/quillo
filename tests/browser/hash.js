// Goes to spots three ways and reports, for each, whether the spot is at the
// top of the window, what :target is, and what url() says. The page must not
// load again on the way (window.__kept stays).
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 200 && !window.__quillo_booted; i++) await sleep(30);
  window.__kept = true;
  const seen = (id) => {
    const el = document.getElementById(id);
    const t = document.querySelector(":target");
    return {
      at: !!el && Math.abs(el.getBoundingClientRect().top) < 2,
      target: t ? t.id : null,
      note: document.getElementById("note").textContent,
    };
  };
  const out = {};

  // A link to a spot on the page, clicked once it is in view.
  document.getElementById("jump").scrollIntoView({ block: "center" });
  document.getElementById("jump").click();
  await sleep(300);
  out.link = seen("target");

  // Back: the spot is left, and url() follows.
  history.back();
  await sleep(300);
  out.back = { hash: location.hash, note: document.getElementById("note").textContent };

  // navigate("#target") from code.
  window.scrollTo(0, 0);
  document.getElementById("same").click();
  await sleep(300);
  out.same = seen("target");

  // navigate("/other#deep"): #deep is there only once /other has rendered.
  window.scrollTo(0, 0);
  document.getElementById("other").click();
  await sleep(500);
  out.other = seen("deep");

  out.kept = window.__kept === true;
  out.errors = window.__quilloErrors || [];
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
