// Feedback from a page's live preview (lib/feedback.qwe): the chosen block,
// copied into the feedback window at its size and scaled to fit, with a
// canvas over it to draw on. Strokes are kept in the block's own pixels,
// so the team sees them where they were drawn, at any size.
(() => {
  let strokes = [];
  let color = "#e11d48";
  let size = { w: 0, h: 0 };
  let canvas = null;

  const block = (id) => document.querySelector(`[data-live-block="${CSS.escape(String(id))}"]`);

  const paint = () => {
    if (!canvas) return;
    const g = canvas.getContext("2d");
    g.clearRect(0, 0, canvas.width, canvas.height);
    g.lineCap = "round";
    g.lineJoin = "round";
    g.lineWidth = 4;
    for (const s of strokes) {
      g.strokeStyle = s.color;
      g.beginPath();
      s.points.forEach(([x, y], i) => (i ? g.lineTo(x, y) : g.moveTo(x, y)));
      if (s.points.length === 1) g.lineTo(s.points[0][0] + 0.1, s.points[0][1]);
      g.stroke();
    }
  };

  const draw = (c) => {
    let current = null;
    const at = (e) => {
      const r = c.getBoundingClientRect();
      return [Math.round(((e.clientX - r.left) / r.width) * size.w), Math.round(((e.clientY - r.top) / r.height) * size.h)];
    };
    c.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      c.setPointerCapture(e.pointerId);
      current = { color, points: [at(e)] };
      strokes.push(current);
      paint();
    });
    c.addEventListener("pointermove", (e) => {
      if (!current) return;
      const p = at(e);
      const last = current.points[current.points.length - 1];
      if (Math.abs(p[0] - last[0]) + Math.abs(p[1] - last[1]) >= 2) { current.points.push(p); paint(); }
    });
    const end = () => { current = null; };
    c.addEventListener("pointerup", end);
    c.addEventListener("pointercancel", end);
  };

  window.pbFeedback = {
    // Puts block `id`, as it is on the page, into the frame (a selector).
    mount(id, frame, tries = 30) {
      requestAnimationFrame(() => {
        const src = block(id);
        const host = document.querySelector(frame);
        if (!src || !host) { if (tries > 0) this.mount(id, frame, tries - 1); return; }
        const r = src.getBoundingClientRect();
        size = { w: Math.max(40, Math.round(r.width)), h: Math.max(20, Math.round(r.height)) };
        strokes = [];
        const scale = Math.min(1, host.clientWidth / size.w);
        host.textContent = "";
        host.style.height = `${Math.round(size.h * scale)}px`;
        const inner = document.createElement("div");
        inner.className = "fb-frame-inner";
        inner.style.width = `${size.w}px`;
        inner.style.transform = `scale(${scale})`;
        const copy = src.cloneNode(true);
        copy.classList.remove("live-focus");
        copy.removeAttribute("data-live-block");
        inner.appendChild(copy);
        canvas = document.createElement("canvas");
        canvas.width = size.w;
        canvas.height = size.h;
        canvas.className = "fb-canvas";
        canvas.setAttribute("aria-label", "Draw on the block");
        inner.appendChild(canvas);
        host.appendChild(inner);
        draw(canvas);
      });
    },
    color(c) { color = c; },
    undo() { strokes.pop(); paint(); },
    clear() { strokes = []; paint(); },
    // What was drawn, and the size it was drawn at.
    take() { return { strokes, width: size.w, height: size.h }; },
    // The viewer's name, kept in this browser.
    name() { try { return localStorage.getItem("relay-feedback-name") || ""; } catch (_) { return ""; } },
    remember(n) { try { localStorage.setItem("relay-feedback-name", n); } catch (_) {} },
  };
})();
