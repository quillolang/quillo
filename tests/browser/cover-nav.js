// Drives tests/browser/cover-nav.qwe at /: navigate() on a page where nothing
// reads url(), to a spot on it (no reload), then to an odd address; /landed
// reports what it was asked for, and what this page saw before.
(async () => {
  const tick = (ms) => new Promise((r) => setTimeout(r, ms));
  for (let i = 0; i < 200 && !window.__quillo_booted; i++) await tick(30);
  const to = document.getElementById("to");
  const go = async (where) => {
    to.value = where;
    to.dispatchEvent(new Event("input", { bubbles: true }));
    await tick(50);
    document.getElementById("go").click();
  };
  // A spot on this page: the browser goes there, the page stays.
  window.__same = true;
  await go("#middle");
  for (let i = 0; i < 100 && window.scrollY < 2000; i++) await tick(20);
  sessionStorage.setItem("before", JSON.stringify({ hash: location.hash, scrolled: window.scrollY > 2000, same: window.__same === true }));
  // A space, an accented letter, a key given twice, a bad escape and a spot.
  await go("/landed/caf\u00e9 x?q=a b&q=last&e=%zz#spot two");
})();
