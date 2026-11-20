// Five input events (typing "hello"), then one change event (leaving the field), as a browser sends them.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  await sleep(1500);
  const input = document.querySelector("input");
  input.focus();
  for (const v of ["h", "he", "hel", "hell", "hello"]) { input.value = v; input.dispatchEvent(new Event("input", { bubbles: true })); await sleep(100); }
  await sleep(300);
  const typed = document.querySelector("p").textContent;
  input.dispatchEvent(new Event("change", { bubbles: true }));
  await sleep(300);
  document.body.setAttribute("data-result", `after 5 input events: n = ${typed}; after a change event: n = ${document.querySelector("p").textContent}`);
})();
