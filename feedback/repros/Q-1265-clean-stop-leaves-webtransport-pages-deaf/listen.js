// Waits for the first line the page hears (the shell posts "1", "2", ... a second apart, from the
// restart on), and says which it was: post N came about N seconds after the restart.
(async () => {
  for (let i = 0; i < 450; i++) {
    const li = document.querySelector("#lines li");
    if (li) { document.body.setAttribute("data-result", `first heard post ${li.textContent}`); return; }
    await new Promise((r) => setTimeout(r, 100));
  }
  document.body.setAttribute("data-result", "heard nothing in 45 s");
})();
