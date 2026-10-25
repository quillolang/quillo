// Review in the browser: what was marked (text selected in a field, or a
// picture clicked), and the comments painted on the record's text as
// highlights. The app (ui/review.qwe) calls these through `js(...)`.
(() => {
  let last = null;

  // The field a node is in, and the text of that field before the node.
  const fieldOf = (node) => {
    const el = node && (node.nodeType === 1 ? node : node.parentElement);
    return el && el.closest("[data-rv-field]");
  };
  const textBefore = (field, node, offset) => {
    const r = document.createRange();
    r.setStart(field, 0);
    r.setEnd(node, offset);
    return r.toString();
  };

  document.addEventListener("selectionchange", () => {
    const sel = document.getSelection();
    if (!sel || sel.isCollapsed || !sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    const field = fieldOf(range.startContainer);
    if (!field || field !== fieldOf(range.endContainer)) return;
    const quote = range.toString();
    if (!quote.trim()) return;
    last = { field: field.dataset.rvField, quote, prefix: textBefore(field, range.startContainer, range.startOffset).slice(-40), image: "" };
    document.querySelectorAll(".rv-picked").forEach((e) => e.classList.remove("rv-picked"));
  });

  document.addEventListener("click", (e) => {
    const img = e.target.closest && e.target.closest("[data-rv-field] img");
    if (img) {
      const field = fieldOf(img);
      last = { field: field.dataset.rvField, quote: "", prefix: "", image: img.getAttribute("src") };
      document.querySelectorAll(".rv-picked").forEach((x) => x.classList.remove("rv-picked"));
      img.classList.add("rv-picked");
      return;
    }
    const mark = e.target.closest && e.target.closest("[data-rv-id]");
    if (mark) {
      const t = document.getElementById("rv-thread-" + mark.dataset.rvId);
      if (t) { t.scrollIntoView({ block: "nearest", behavior: "smooth" }); t.classList.add("rv-flash"); setTimeout(() => t.classList.remove("rv-flash"), 900); }
    }
  });

  // A thread hovered lights its mark up.
  document.addEventListener("mouseover", (e) => {
    const t = e.target.closest && e.target.closest(".rv-thread");
    document.querySelectorAll(".rv-lit").forEach((x) => x.classList.remove("rv-lit"));
    if (t) document.querySelectorAll(`[data-rv-id="${t.dataset.id}"]`).forEach((x) => x.classList.add("rv-lit"));
  });

  // The text nodes of a field, with where each starts in its text.
  const texts = (root) => {
    const out = [];
    let at = 0;
    const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) { out.push({ n, at }); at += n.data.length; }
    return out;
  };

  // Wraps the text from `start` to `end` (offsets in the field's text) in
  // marks, one per text node it crosses.
  const wrap = (root, start, end, id, resolved) => {
    for (const { n, at } of texts(root)) {
      // The node's length before it is split (splitting shortens it).
      const len = n.data.length;
      const a = Math.max(start, at), b = Math.min(end, at + len);
      if (a >= b) continue;
      let node = n;
      if (a > at) node = node.splitText(a - at);
      if (b < at + len) node.splitText(b - a);
      const m = document.createElement("mark");
      m.className = resolved ? "rv-mark rv-resolved" : "rv-mark";
      m.dataset.rvId = id;
      node.parentNode.insertBefore(m, node);
      m.appendChild(node);
    }
  };

  window.relayReview = {
    // What was marked last (null: nothing), and forgetting it.
    last: () => last,
    clear: () => { last = null; document.querySelectorAll(".rv-picked").forEach((x) => x.classList.remove("rv-picked")); },
    // Paints the comments ([{ id, field, quote, prefix, image, resolved }])
    // on the record's fields in `root`.
    paint: (root, notes) => {
      for (const note of notes || []) {
        const field = root.querySelector(`[data-rv-field="${CSS.escape(note.field)}"]`);
        if (!field) continue;
        if (note.image) {
          for (const img of field.querySelectorAll("img")) if (img.getAttribute("src") === note.image) { img.dataset.rvId = note.id; img.classList.add("rv-img"); if (note.resolved) img.classList.add("rv-resolved"); }
          continue;
        }
        if (!note.quote) continue;
        const text = field.textContent;
        // The occurrence whose text before it ends as it did.
        let best = -1;
        for (let i = text.indexOf(note.quote); i >= 0; i = text.indexOf(note.quote, i + 1)) {
          if (best < 0) best = i;
          if (note.prefix && text.slice(0, i).endsWith(note.prefix)) { best = i; break; }
        }
        if (best >= 0) wrap(field, best, best + note.quote.length, note.id, note.resolved);
      }
      return null;
    },
  };
})();
