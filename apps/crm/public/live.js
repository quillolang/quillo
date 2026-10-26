// A page's live preview follows its designer: when the page changes, the
// block being edited (.live-focus) is brought into view if it is out of it.
(() => {
  let last = null;
  const follow = () => {
    const el = document.querySelector(".live-focus");
    const id = el && el.dataset.liveBlock;
    if (!el || id === last) return;
    last = id;
    const r = el.getBoundingClientRect();
    if (r.bottom < 0 || r.top > innerHeight) el.scrollIntoView({ block: "center", behavior: "smooth" });
  };
  const start = () => {
    const root = document.querySelector(".site-main") || document.body;
    new MutationObserver(follow).observe(root, { childList: true, subtree: true });
    follow();
  };
  document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", start) : start();
})();
