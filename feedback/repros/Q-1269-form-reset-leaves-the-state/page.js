// Types "Ana" (an input event, as typing gives), clicks Clear, and says what the field and the text show.
(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  await sleep(1500);
  const input = document.querySelector("input");
  input.focus(); input.value = "Ana"; input.dispatchEvent(new Event("input", { bubbles: true }));
  await sleep(500);
  const typed = document.querySelector("p").textContent;
  document.querySelector("button").click();
  await sleep(500);
  document.body.setAttribute("data-result", `typed: text "${typed}"; after Clear: input "${input.value}", text "${document.querySelector("p").textContent}"`);
})();
