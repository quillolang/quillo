// For mobile/ios/build.sh's screenshots: adds three to-dos to app/main.qwe's page
// through its own form, as a person would, and ticks the first.
(async () => {
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const input = () => document.querySelector("form input");
  input().dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
  for (let i = 0; i < 60 && !window.__quillo_booted; i++) await wait(50);
  await wait(200);
  for (const text of ["Buy milk", "Ship the iOS build", "Write the mobile docs"]) {
    const box = input();
    box.value = text;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    document.querySelector("form button").click();
    for (let i = 0; i < 40 && ![...document.querySelectorAll("li")].some((l) => l.textContent.includes(text)); i++) await wait(50);
  }
  const first = document.querySelector("li input[type=checkbox]");
  if (first) first.click();
  await wait(300);
  document.activeElement && document.activeElement.blur();
})();
