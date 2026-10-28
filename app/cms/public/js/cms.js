// Plain JavaScript beside the components (ui/page.qwe loads it). Components
// call into it by name: js("cms.reveal", id).
window.cms = {
  // Scrolls a block into view and flashes it, once it is on the page.
  reveal(id) {
    requestAnimationFrame(() => {
      const el = document.getElementById(id);
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      el.classList.remove("flash");
      void el.offsetWidth;
      el.classList.add("flash");
    });
  },
};
