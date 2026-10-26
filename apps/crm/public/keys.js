// Keys for the whole app: ⌘K (Ctrl+K elsewhere) goes to the search.
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
    const input = document.querySelector(".search input");
    if (input) { e.preventDefault(); input.focus(); input.select(); }
  }
});

// Focus once the page is drawn again: the page designer keeps the keyboard
// on the block chosen (js("relayFocus", selector)).
window.relayFocus = (selector) => requestAnimationFrame(() => {
  const el = document.querySelector(selector);
  if (el) el.focus({ preventScroll: false });
});
