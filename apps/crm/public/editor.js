// The page designer's editing in place, as in Gutenberg: what a block's HTML
// marks with data-edit (a field's key; data-md="1": Markdown, "inline": a
// line of it, bold to links but no headings or lists) is typed into where
// it shows; Enter is the next paragraph of the same block. The designer
// (ui/pagedesigner.qwe) hears it from the element:
// `input` (a value changed: pbEditor.value(el) reads it), `focusout` (done:
// draw it again), and events of its own, each with { key, before, rest }:
//   pbback   Backspace in an empty one: remove it
//   pbslash  "/" in an empty one: choose a block for it
// In Markdown, a toolbar formats what is chosen: bold, italic, code, links,
// headings, quotes, lists (⌘B, ⌘I, ⌘K; "## ", "- ", "1. ", "> " typed at
// the start of a line).
(() => {
  // The marked element a node is in (a text node: its parent's).
  const editable = (t) => {
    const el = t && (t.nodeType === 1 ? t : t.parentElement);
    return el && el.closest ? el.closest(".pb-html [data-edit]") : null;
  };

  // Links in the preview lead nowhere while the page is designed.
  document.addEventListener("click", (e) => {
    if (e.target.closest && e.target.closest(".pb-html a")) e.preventDefault();
  }, true);

  const open = (el) => {
    if (el.isContentEditable) return;
    if (el.dataset.md) el.contentEditable = "true";
    else {
      try { el.contentEditable = "plaintext-only"; } catch (_) { el.contentEditable = "true"; }
    }
    el.spellcheck = true;
  };
  const close = (el) => { el.removeAttribute("contenteditable"); };

  // Pressed: typed into from there (the caret lands where the press was).
  document.addEventListener("pointerdown", (e) => {
    const el = editable(e.target);
    if (el) open(el);
  }, true);
  document.addEventListener("focusout", (e) => {
    const el = editable(e.target);
    if (el) setTimeout(() => { if (document.activeElement !== el && !bar.contains(document.activeElement)) { close(el); hide(); } }, 0);
  }, true);

  // ---- Markdown, back from the HTML typed into --------------------------------

  const inline = (n) => {
    if (n.nodeType === 3) return n.data.replace(/\s+/g, " ");
    if (n.nodeType !== 1) return "";
    const inner = [...n.childNodes].map(inline).join("");
    switch (n.tagName) {
      case "STRONG": case "B": return inner.trim() ? `**${inner.trim()}**` : inner;
      case "EM": case "I": return inner.trim() ? `*${inner.trim()}*` : inner;
      case "CODE": return "`" + n.textContent + "`";
      case "A": return `[${inner}](${n.getAttribute("href") || ""})`;
      case "BR": return "\n";
      default: return inner;
    }
  };
  const BLOCKS = /^(P|DIV|H[1-6]|UL|OL|LI|BLOCKQUOTE|PRE|HR|SECTION)$/;
  const LISTS = /^(UL|OL)$/;
  // An element holding blocks (a list in a paragraph, as browsers make): its blocks.
  const holdsBlocks = (n) => [...n.children].some((c) => BLOCKS.test(c.tagName));
  // A list, its items' own text, and the lists in them indented below.
  const listMd = (list, depth) => [...list.children].filter((li) => li.tagName === "LI").map((li, i) => {
    const own = [...li.childNodes].filter((c) => !(c.nodeType === 1 && LISTS.test(c.tagName))).map(inline).join("").trim();
    const subs = [...li.children].filter((c) => LISTS.test(c.tagName)).map((sub) => listMd(sub, depth + 1));
    return "   ".repeat(depth) + (list.tagName === "OL" ? `${i + 1}. ` : "- ") + own + subs.map((x) => "\n" + x).join("");
  }).join("\n");
  const blocks = (root) => [...root.childNodes].flatMap((n) => {
    if (n.nodeType === 3) { const s = n.data.trim(); return s ? [s] : []; }
    if (n.nodeType !== 1) return [];
    const t = n.tagName;
    if (LISTS.test(t)) return [listMd(n, 0)];
    if (/^H[1-6]$/.test(t)) return holdsBlocks(n) ? blocks(n) : ["#".repeat(+t[1]) + " " + inline(n).trim()];
    if (t === "BLOCKQUOTE") return [blocks(n).join("\n\n").split("\n").map((l) => "> " + l).join("\n")];
    if (t === "PRE") return ["```\n" + n.textContent.replace(/\n$/, "") + "\n```"];
    if (t === "HR") return ["---"];
    if (t === "LI") return [listMd({ tagName: "UL", children: [n] }, 0)];
    if (holdsBlocks(n)) return blocks(n);
    const s = inline(n).trim();
    return s ? [s] : [];
  });
  const markdown = (root) => blocks(root).join("\n\n").replace(/\n{3,}/g, "\n\n").trim();
  // A line: its inline Markdown, a line break kept as one.
  const lineMd = (root) => [...root.childNodes].map(inline).join("").replace(/\n/g, "  \n").trim();
  const plain = (root) => root.innerText.replace(/ /g, " ").replace(/\n+$/, "");

  // What a marked element holds: Markdown, or its text.
  // A list typed into (data-md="lines"): one item a line.
  const items = (root) => root.innerText.split("\n").map((x) => x.replace(/\u00a0/g, " ").trim()).filter((x) => x !== "").join("\n");
  const value = (el) => (el.dataset.md === "inline" ? lineMd(el) : el.dataset.md === "lines" ? items(el) : el.dataset.md ? markdown(el) : plain(el));

  // ---- the keys that make and remove blocks -------------------------------------

  const tell = (el, type, detail) => el.dispatchEvent(new CustomEvent(type, { bubbles: true, detail: { key: el.dataset.edit, ...detail } }));

  document.addEventListener("keydown", (e) => {
    const el = editable(e.target);
    if (!el || !el.isContentEditable || e.isComposing) return;
    // In a list of lines, Enter is the next item, as typed.
    if (el.dataset.md === "lines") { if (e.key === "Escape") el.blur(); return; }
    if ((e.metaKey || e.ctrlKey) && !e.altKey && el.dataset.md) {
      const k = e.key.toLowerCase();
      if (k === "b" || k === "i") { e.preventDefault(); cur = el; apply(k === "b" ? "bold" : "italic"); return; }
      if (k === "k") { e.preventDefault(); e.stopPropagation(); cur = el; openLink(); return; }
    }
    const empty = value(el).trim() === "";
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      // A line of its own (a headline, a caption, a label): Enter is done.
      if (el.dataset.md !== "1") { el.blur(); return; }
      newLine(el);
    } else if (e.key === "Backspace" && empty && el.dataset.md === "1") {
      e.preventDefault();
      tell(el, "pbback", {});
    } else if (e.key === "/" && empty && el.dataset.md === "1") {
      e.preventDefault();
      tell(el, "pbslash", {});
    } else if (e.key === "Escape") {
      el.blur();
    }
  }, true);

  // Pasted: as text, not the HTML of wherever it came from.
  document.addEventListener("paste", (e) => {
    const el = editable(e.target);
    if (!el || !el.isContentEditable) return;
    e.preventDefault();
    const text = (e.clipboardData || window.clipboardData).getData("text/plain");
    document.execCommand("insertText", false, text);
  }, true);

  // Enter in Markdown: the next paragraph, in the same block. In a list,
  // the next item; on an empty last one, a paragraph after the list. In a
  // quote, the next line of it; after a heading, a paragraph.
  const newLine = (el) => {
    cur = el;
    tidy(el);
    const s = getSelection();
    if (!s.rangeCount) return;
    const r = s.getRangeAt(0);
    if (!r.collapsed) r.deleteContents();
    const line = lineOf(el);
    if (!line) return;
    const empty = (n) => !n.textContent.replace(/\u00a0/g, " ").trim() && !n.querySelector("img");
    if (line.tagName === "LI" && empty(line) && !line.nextElementSibling) {
      const list = line.parentElement;
      const p = document.createElement("p");
      p.appendChild(document.createElement("br"));
      (list.parentElement.closest("li") || list).after(p);
      line.remove();
      if (!list.children.length) list.remove();
      caretTo(p, 0);
      changed();
      return;
    }
    const tail = document.createRange();
    tail.selectNodeContents(line);
    tail.setStart(r.endContainer, r.endOffset);
    const rest = tail.extractContents();
    const next = document.createElement(line.tagName === "LI" ? "li" : line.tagName === "BLOCKQUOTE" ? "blockquote" : "p");
    next.appendChild(rest);
    // An empty side keeps a line break, so the caret has a place in it.
    for (const n of [line, next]) if (!n.textContent && !n.querySelector("br, img")) n.appendChild(document.createElement("br"));
    line.after(next);
    caretTo(next, 0);
    next.scrollIntoView({ block: "nearest" });
    changed();
  };

  // ---- formatting -------------------------------------------------------------------

  // What a line's text (data-md="inline") cannot have: these are left out for it.
  const BLOCK_CMDS = ["h2", "h3", "p", "quote", "ul", "ol"];
  const BUTTONS = [
    ["bold", "B", "Bold (⌘B)"], ["italic", "I", "Italic (⌘I)"], ["code", "‹›", "Code"], ["link", "Link", "Link (⌘K)"], null,
    ["h2", "H2", "Heading (## )"], ["h3", "H3", "Small heading (### )"], ["p", "¶", "Text"], ["quote", "❝", "Quote (> )"],
    ["ul", "•", "Bulleted list (- )"], ["ol", "1.", "Numbered list (1. )"], null, ["clear", "Tx", "Clear formatting"],
  ];
  const bar = document.createElement("div");
  bar.className = "pb-format";
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", "Formatting");
  bar.hidden = true;
  const tools = document.createElement("div");
  tools.className = "pb-format-tools";
  for (const b of BUTTONS) {
    if (!b) { const sep = document.createElement("span"); sep.className = "pb-format-sep"; sep.dataset.after = tools.children.length > 5 ? "block" : "inline"; tools.appendChild(sep); continue; }
    const btn = document.createElement("button");
    btn.type = "button"; btn.dataset.cmd = b[0]; btn.textContent = b[1]; btn.title = b[2];
    if (BLOCK_CMDS.includes(b[0])) btn.dataset.kind = "block";
    btn.setAttribute("aria-label", b[2]); btn.setAttribute("aria-pressed", "false");
    tools.appendChild(btn);
  }
  const linkForm = document.createElement("form");
  linkForm.className = "pb-link";
  linkForm.hidden = true;
  linkForm.innerHTML = '<input type="text" aria-label="Link address" placeholder="/a-page, https://…, mailto:…"><button type="submit">Apply</button><button type="button" data-unlink>Remove link</button><span class="pb-link-bad" hidden>Starts with /, https://, http://, mailto: or #</span>';
  bar.append(tools, linkForm);
  const linkInput = linkForm.querySelector("input");
  const ready = () => { if (!bar.isConnected) document.body.appendChild(bar); };
  document.readyState === "loading" ? document.addEventListener("DOMContentLoaded", ready) : ready();

  let cur = null;       // the element being formatted
  let saved = null;     // the selection, while the link's address is typed

  const within = (node, sel) => {
    const el = node && (node.nodeType === 1 ? node : node.parentElement);
    const hit = el && el.closest(sel);
    return hit && cur && cur.contains(hit) && hit !== cur ? hit : null;
  };
  const anchor = () => { const s = getSelection(); return s.rangeCount ? s.getRangeAt(0).commonAncestorContainer : null; };
  const is = (sel) => !!within(anchor(), sel);
  const changed = () => cur && cur.dispatchEvent(new Event("input", { bubbles: true }));

  // ---- lines made headings, quotes and lists (by hand: the browser's own
  // commands nest them in paragraphs inside a field) ---------------------------
  // Text lying loose in the field: in a paragraph, so each line is one.
  const tidy = (el) => {
    let run = [];
    const wrap = () => {
      if (run.length && run.some((n) => n.nodeType !== 3 || n.data.trim())) { const p = document.createElement("p"); run[0].before(p); p.append(...run); }
      run = [];
    };
    for (const n of [...el.childNodes]) { if (n.nodeType === 1 && BLOCKS.test(n.tagName)) wrap(); else run.push(n); }
    wrap();
    // A list or a heading in a paragraph: taken out of it.
    for (const p of [...el.querySelectorAll(":scope > p, :scope > h2, :scope > h3, :scope > blockquote")]) {
      if (![...p.children].some((c) => LISTS.test(c.tagName))) continue;
      for (const c of [...p.childNodes]) p.before(c.nodeType === 3 && c.data.trim() ? Object.assign(document.createElement("p"), { textContent: c.data }) : c);
      p.remove();
    }
  };
  // The line the caret is in: an item, or a block of the field.
  const lineOf = (el) => {
    const n = editable(anchor()) === el ? anchor() : null;
    const e = n && (n.nodeType === 1 ? n : n.parentElement);
    if (!e) return null;
    const li = e.closest("li");
    if (li && el.contains(li)) return li;
    let x = e;
    while (x && x.parentElement !== el) x = x.parentElement;
    return x && x !== el ? x : null;
  };
  // Where the caret is in a line, as a count of characters, and put back.
  const caretAt = (line) => {
    const s = getSelection();
    if (!s.rangeCount) return 0;
    const r = document.createRange();
    r.selectNodeContents(line);
    r.setEnd(s.getRangeAt(0).endContainer, s.getRangeAt(0).endOffset);
    return r.toString().length;
  };
  const caretTo = (line, at) => {
    const w = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
    let n, left = at;
    const r = document.createRange();
    while ((n = w.nextNode())) { if (left <= n.data.length) { r.setStart(n, left); break; } left -= n.data.length; }
    if (!n) { r.selectNodeContents(line); r.collapse(false); } else r.collapse(true);
    const s = getSelection(); s.removeAllRanges(); s.addRange(r);
  };
  const ownNodes = (li) => [...li.childNodes].filter((c) => !(c.nodeType === 1 && LISTS.test(c.tagName)));
  // An item out of its list, as a block of `tag` (the list split around it).
  const liftItem = (li, tag) => {
    const list = li.parentElement;
    const after = [...list.children].slice([...list.children].indexOf(li) + 1);
    const out = document.createElement(tag);
    out.append(...ownNodes(li));
    list.after(out);
    if (after.length) { const rest = document.createElement(list.tagName); rest.append(...after); out.after(rest); }
    li.remove();
    if (!list.children.length) list.remove();
    return out;
  };
  const block = (tag) => {
    tidy(cur);
    const line = lineOf(cur);
    if (!line) return;
    const at = caretAt(line);
    let out;
    if (line.tagName === "LI") out = liftItem(line, tag);
    else if (line.tagName.toLowerCase() === tag) return;
    else { out = document.createElement(tag); out.append(...line.childNodes); line.replaceWith(out); }
    if (!out.firstChild) out.appendChild(document.createElement("br"));
    caretTo(out, at);
  };
  const listOf = (kind) => {
    tidy(cur);
    const line = lineOf(cur);
    if (!line) return;
    const at = caretAt(line);
    const tag = kind.toUpperCase();
    if (line.tagName === "LI") {
      const list = line.parentElement;
      if (list.tagName === tag) { caretTo(liftItem(line, "p"), at); return; }
      const other = document.createElement(kind);
      other.append(...list.childNodes);
      list.replaceWith(other);
      caretTo(line, at);
      return;
    }
    const li = document.createElement("li");
    li.append(...line.childNodes);
    if (!li.firstChild) li.appendChild(document.createElement("br"));
    const prev = line.previousElementSibling, next = line.nextElementSibling;
    if (prev && prev.tagName === tag) { prev.appendChild(li); line.remove(); }
    else { const list = document.createElement(kind); list.appendChild(li); line.replaceWith(list); }
    // Joined with the list after it, if it is the same kind.
    const list = li.parentElement;
    if (next && next.tagName === tag && next !== list) { list.append(...next.childNodes); next.remove(); }
    caretTo(li, at);
  };

  const hide = () => { bar.hidden = true; closeLink(false); };
  // Beside the block's own toolbar (or just under it, when there is no room
  // beside it); over the choice when that toolbar has scrolled away.
  const place = () => {
    const s = getSelection();
    if (!cur || !s.rangeCount) return;
    const w0 = bar.offsetWidth;
    const own = cur.closest(".pb-wrap") && cur.closest(".pb-wrap").querySelector(".pb-toolbar");
    const t = own && own.getBoundingClientRect();
    if (t && t.top >= 64 && t.bottom <= innerHeight) {
      let left = t.right + 8, top = t.top + (t.height - bar.offsetHeight) / 2;
      if (left + w0 > innerWidth - 8) { left = t.left; top = t.bottom + 6; }
      bar.style.top = `${Math.round(top)}px`;
      bar.style.left = `${Math.round(Math.max(8, left))}px`;
      return;
    }
    let r = s.getRangeAt(0).getBoundingClientRect();
    if (!r.width && !r.height) r = (s.anchorNode.nodeType === 1 ? s.anchorNode : s.anchorNode.parentElement).getBoundingClientRect();
    const w = bar.offsetWidth, h = bar.offsetHeight;
    let top = r.top - h - 10;
    if (top < 64) top = r.bottom + 10;
    bar.style.top = `${Math.round(top)}px`;
    bar.style.left = `${Math.round(Math.min(Math.max(8, r.left + r.width / 2 - w / 2), innerWidth - w - 8))}px`;
  };
  const refresh = () => {
    const state = {
      bold: is("strong, b"), italic: is("em, i"), code: is("code"), link: is("a"),
      h2: is("h2"), h3: is("h3"), quote: is("blockquote"), ul: is("ul"), ol: is("ol"),
    };
    state.p = !state.h2 && !state.h3 && !state.quote && !state.ul && !state.ol;
    for (const btn of tools.querySelectorAll("button")) btn.setAttribute("aria-pressed", String(!!state[btn.dataset.cmd]));
  };

  // The caret or a choice in Markdown: the toolbar, over it.
  document.addEventListener("selectionchange", () => {
    if (!linkForm.hidden) return;
    const el = editable(anchor());
    if (!el || !el.isContentEditable || !["1", "inline"].includes(el.dataset.md)) { if (!bar.contains(document.activeElement)) bar.hidden = true; return; }
    cur = el;
    ready();
    bar.classList.toggle("line-only", el.dataset.md === "inline");
    bar.hidden = false;
    refresh();
    place();
  });
  addEventListener("scroll", () => { if (!bar.hidden) place(); }, true);
  addEventListener("resize", () => { if (!bar.hidden) place(); });

  // Bold and italic by their elements, not the browser's commands: those
  // read a headline (bold already, by its style) as bold, and undo it.
  const toggleInline = (tag, alike) => {
    const s = getSelection();
    const on = within(anchor(), alike);
    if (on) { on.replaceWith(...on.childNodes); return; }
    if (!s.rangeCount || s.isCollapsed) return;
    const r = s.getRangeAt(0);
    const wrap = document.createElement(tag);
    wrap.appendChild(r.extractContents());
    // What was already bold inside it is just text in it now.
    for (const x of [...wrap.querySelectorAll(alike)]) x.replaceWith(...x.childNodes);
    r.insertNode(wrap);
    const again = document.createRange();
    again.selectNodeContents(wrap);
    s.removeAllRanges();
    s.addRange(again);
  };
  const toggleCode = () => {
    const s = getSelection();
    const code = within(anchor(), "code");
    if (code) {
      code.replaceWith(...code.childNodes);
    } else if (s.rangeCount && !s.isCollapsed) {
      const text = s.toString().replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
      document.execCommand("insertHTML", false, `<code>${text}</code>`);
    }
  };
  const apply = (cmd) => {
    if (!cur) return;
    if (document.activeElement !== cur) cur.focus();
    switch (cmd) {
      case "bold": toggleInline("strong", "strong, b"); break;
      case "italic": toggleInline("em", "em, i"); break;
      case "code": toggleCode(); break;
      case "link": openLink(); return;
      case "h2": case "h3": block(is(cmd) ? "p" : cmd); break;
      case "p": block("p"); break;
      case "quote": block(is("blockquote") ? "p" : "blockquote"); break;
      case "ul": listOf("ul"); break;
      case "ol": listOf("ol"); break;
      case "clear": document.execCommand("removeFormat"); document.execCommand("unlink"); break;
    }
    changed();
    refresh();
    place();
  };
  // Pressing a button keeps the choice where it is.
  tools.addEventListener("mousedown", (e) => { if (e.target.closest("button")) e.preventDefault(); });
  tools.addEventListener("click", (e) => { const b = e.target.closest("button"); if (b) apply(b.dataset.cmd); });

  // ---- links ---------------------------------------------------------------------
  const LINK_OK = /^(\/|https?:\/\/|mailto:|#)/i;
  const openLink = () => {
    const s = getSelection();
    if (!cur || !s.rangeCount) return;
    saved = s.getRangeAt(0).cloneRange();
    const a = within(anchor(), "a");
    linkInput.value = a ? a.getAttribute("href") || "" : "";
    linkForm.querySelector("[data-unlink]").hidden = !a;
    linkForm.querySelector(".pb-link-bad").hidden = true;
    tools.hidden = true;
    linkForm.hidden = false;
    bar.hidden = false;
    place();
    linkInput.focus();
    linkInput.select();
  };
  const closeLink = (back) => {
    if (linkForm.hidden) return;
    linkForm.hidden = true;
    tools.hidden = false;
    if (back && cur && saved) { cur.focus(); const s = getSelection(); s.removeAllRanges(); s.addRange(saved); }
    saved = null;
  };
  const restore = () => { cur.focus(); const s = getSelection(); s.removeAllRanges(); s.addRange(saved); };
  linkForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const href = linkInput.value.trim();
    if (!href || !LINK_OK.test(href)) { linkForm.querySelector(".pb-link-bad").hidden = false; linkInput.focus(); return; }
    restore();
    const a = within(anchor(), "a");
    if (a) a.setAttribute("href", href);
    else if (saved.collapsed) {
      const t = href.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
      document.execCommand("insertHTML", false, `<a href="${t}">${t}</a>`);
    } else document.execCommand("createLink", false, href);
    changed();
    closeLink(false);
    refresh();
  });
  linkForm.querySelector("[data-unlink]").addEventListener("click", () => {
    restore();
    const a = within(anchor(), "a");
    if (a) { const r = document.createRange(); r.selectNodeContents(a); const s = getSelection(); s.removeAllRanges(); s.addRange(r); document.execCommand("unlink"); }
    changed();
    closeLink(false);
    refresh();
  });
  linkInput.addEventListener("keydown", (e) => { if (e.key === "Escape") { e.preventDefault(); closeLink(true); } });

  // ---- Markdown typed at the start of a line --------------------------------------
  // "## " makes the line a heading ("### " a small one), "- " or "* " a
  // list, "1. " a numbered one, "> " a quote: the marks go, the line turns.
  const SHORTCUTS = [[/^###\s$/, "h3"], [/^##\s$/, "h2"], [/^[-*]\s$/, "ul"], [/^1\.\s$/, "ol"], [/^>\s$/, "blockquote"]];
  document.addEventListener("input", (e) => {
    const el = editable(e.target);
    if (!el || el.dataset.md !== "1" || !String(e.inputType || "").startsWith("insert")) return;
    const s = getSelection();
    if (!s.rangeCount || !s.isCollapsed) return;
    const node = s.anchorNode;
    if (!node || node.nodeType !== 3) return;
    // What is before the caret on its line: from the line's start, or from a line break.
    let start = node.previousSibling;
    while (start && !(start.nodeType === 1 && start.tagName === "BR") && start.nodeType === 3 && !start.data) start = start.previousSibling;
    const afterBreak = start && start.nodeType === 1 && start.tagName === "BR";
    if (!afterBreak && start) return;
    const before = node.data.slice(0, s.anchorOffset).replace(/\u00a0/g, " ");
    const hit = SHORTCUTS.find(([re]) => re.test(before));
    if (!hit) return;
    cur = el;
    // The marks go; after a line break, what follows it is a line of its own.
    node.data = node.data.slice(s.anchorOffset);
    if (afterBreak) {
      const holder = start.parentElement;
      if (holder.tagName === "LI") return;
      const rest = document.createElement("p");
      let n = start.nextSibling;
      while (n) { const nx = n.nextSibling; rest.appendChild(n); n = nx; }
      if (holder === el) start.replaceWith(rest); else { holder.after(rest); start.remove(); }
    }
    const r = document.createRange();
    r.setStart(node, 0);
    r.collapse(true);
    s.removeAllRanges();
    s.addRange(r);
    if (hit[1] === "ul" || hit[1] === "ol") listOf(hit[1]); else block(hit[1]);
    changed();
  }, true);

  // ---- for the designer ------------------------------------------------------------

  const caret = (el, where) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    r.collapse(where !== "end");
    const s = getSelection();
    s.removeAllRanges();
    s.addRange(r);
  };

  window.pbEditor = {
    value,
    // Puts the caret in a block's first (or last) marked field, once it is drawn.
    focus(id, where = "start", tries = 30) {
      requestAnimationFrame(() => {
        const wrap = document.querySelector(`[data-block="${CSS.escape(String(id))}"]`);
        const all = wrap ? wrap.querySelectorAll(".pb-html [data-edit]") : [];
        const el = all.length ? (where === "end" ? all[all.length - 1] : all[0]) : null;
        if (!el) { if (tries > 0) this.focus(id, where, tries - 1); else if (wrap) wrap.focus(); return; }
        open(el);
        el.focus({ preventScroll: true });
        caret(el, where);
        el.scrollIntoView({ block: "nearest" });
      });
    },
    // Puts the caret in one field of a block, once it shows.
    focusField(id, key, tries = 30) {
      requestAnimationFrame(() => {
        const el = document.querySelector(`[data-block="${CSS.escape(String(id))}"] .pb-html [data-edit="${CSS.escape(String(key))}"]`);
        if (!el || !el.getClientRects().length) { if (tries > 0) this.focusField(id, key, tries - 1); return; }
        open(el);
        el.focus({ preventScroll: true });
        caret(el, "start");
      });
    },
    // Shows a block (from the outline).
    show(id) {
      requestAnimationFrame(() => {
        const wrap = document.querySelector(`[data-block="${CSS.escape(String(id))}"]`);
        if (wrap) { wrap.scrollIntoView({ block: "center", behavior: "smooth" }); wrap.focus({ preventScroll: true }); }
      });
    },
  };
})();
