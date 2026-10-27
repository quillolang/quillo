// Plain JavaScript, loaded by the page beside the components (ui/page.qwe).
// A component calls into it by name: js("todoApp.showLeft", n) in ui/todos.qwe.

window.todoApp = {
  // How many todos are left, in the tab's title.
  showLeft(left) {
    document.title = left > 0 ? `(${left}) My Quillo app` : "My Quillo app";
  },
};

// Press "/" anywhere to jump to the input.
document.addEventListener("keydown", (e) => {
  if (e.key === "/" && !["INPUT", "TEXTAREA"].includes(document.activeElement.tagName)) {
    e.preventDefault();
    document.querySelector("input[placeholder]")?.focus();
  }
});
