// Clicks the button once the page has booted; /results reports what it got.
(async () => {
  for (let i = 0; i < 200 && !(window[Symbol.for("quillo")] || {}).booted; i++) await new Promise((r) => setTimeout(r, 30));
  document.getElementById("go").click();
})();
