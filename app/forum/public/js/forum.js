// Plain JavaScript beside the components (ui/page.qwe loads it). Components
// call into it by name: js("forum.focusReply") in ui/thread.qwe.

window.forum = {
  // After a quote: bring the reply box into view, with the cursor at its end.
  focusReply() {
    requestAnimationFrame(() => {
      const box = document.getElementById("reply-box");
      if (!box) return;
      box.scrollIntoView({ behavior: "smooth", block: "center" });
      box.focus();
      box.setSelectionRange(box.value.length, box.value.length);
    });
  },
  go(path) {
    location.assign(path);
  },
  // The editor (ui/editor.qwe): where the cursor is in a box, and putting it
  // somewhere once the box shows its new text.
  caretOf(id) {
    const box = document.getElementById(id);
    return box ? box.selectionStart : null;
  },
  caret(id, at) {
    requestAnimationFrame(() => {
      const box = document.getElementById(id);
      if (!box) return;
      box.focus();
      box.setSelectionRange(at, at);
    });
  },
  // A conversation (ui/messages.qwe): show the newest message.
  scrollChat() {
    requestAnimationFrame(() => {
      const list = document.getElementById("chat");
      if (list) list.scrollTop = list.scrollHeight;
    });
  },
};

// "/" jumps to search, from anywhere but a text field.
document.addEventListener("keydown", (e) => {
  const typing = ["INPUT", "TEXTAREA", "SELECT"].includes(document.activeElement.tagName);
  if (e.key === "/" && !typing && !e.metaKey && !e.ctrlKey) {
    e.preventDefault();
    document.querySelector(".search input")?.focus();
  }
});

// A copy button on every code block in a post, including ones that arrive live.
function addCopyButtons(root) {
  for (const pre of root.querySelectorAll(".body pre:not([data-copy])")) {
    pre.dataset.copy = "1";
    const b = document.createElement("button");
    b.type = "button";
    b.className = "copy";
    b.textContent = "Copy";
    b.addEventListener("click", async () => {
      await navigator.clipboard.writeText(pre.querySelector("code")?.innerText ?? pre.innerText);
      b.textContent = "Copied";
      setTimeout(() => (b.textContent = "Copy"), 1200);
    });
    pre.appendChild(b);
  }
}
addCopyButtons(document);
new MutationObserver(() => addCopyButtons(document)).observe(document.body, { childList: true, subtree: true });
