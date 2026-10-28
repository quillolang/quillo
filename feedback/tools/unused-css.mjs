// Lists the CSS classes a program styles but never writes: the selectors of
// its style sheets (public/**/*.css and css`...` in .qwe files) against the
// class names its .qwe files can make.
//
//   node feedback/tools/unused-css.mjs app/bank [--all]
//
// A class counts as written when its name is in the program as a word of a
// string or in markup's shorthand (<div.card>): `class={on && "on"}` and
// `let cls = "card big"` both write theirs. A template's fixed part
// (`ui-tabs-${variant}`) makes every class starting `ui-tabs-` "maybe":
// which ones it makes is known only when the page runs. Quillo's own (ask's
// and notify's q-*, highlight's hl-*) are read from its runtime. Classes a
// script sets (a library's own, Leaflet's .leaflet-*) look unused here;
// --all lists the others too.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = process.argv[2];
if (!root) {
  console.error("usage: unused-css.mjs <program folder> [--all]");
  process.exit(2);
}
const all = process.argv.includes("--all");

const files = (dir) => readdirSync(dir).flatMap((n) => {
  const p = join(dir, n);
  return statSync(p).isDirectory() ? (n === "node_modules" || n.startsWith(".") ? [] : files(p)) : [p];
});
const everything = files(root);
const qwe = everything.filter((p) => p.endsWith(".qwe"));
// The program's own scripts (public/js/...) set classes too: classList.add("flash").
const scripts = everything.filter((p) => p.endsWith(".js") || p.endsWith(".mjs")).filter((p) => !/[/\\]tests[/\\]/.test(p));
const sheets = everything.filter((p) => p.endsWith(".css"));

// css`...` blocks, and the .qwe source without them (their selectors are
// not uses).
const CSS_BLOCK = /css`([\s\S]*?)`/g;

// The classes of a style sheet's selectors, each with where it is.
// `first` is the line the text starts on in its file (a css`...` block's).
function selectorClasses(text, file, out, first = 1) {
  text = text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\$\{[^}]*\}/g, "");
  // A selector is what comes before a `{`; @media and the like are not.
  const re = /([^{}]+)\{/g;
  let m;
  while ((m = re.exec(text))) {
    const sel = m[1].trim();
    if (sel.startsWith("@") || sel === "") continue;
    // Strings and url(...) in a selector ([href$=".pdf"]) are not classes.
    const bare = sel.replace(/"[^"]*"|'[^']*'|url\([^)]*\)/g, "");
    const line = first - 1 + text.slice(0, m.index + m[0].length).split("\n").length;
    for (const c of bare.matchAll(/\.(-?[_a-zA-Z][\w-]*)/g)) {
      if (!out.has(c[1])) out.set(c[1], `${relative(root, file)}:${line}`);
    }
  }
}

const styled = new Map();
for (const f of sheets) selectorClasses(readFileSync(f, "utf8"), f, styled);
for (const f of qwe) {
  const src = readFileSync(f, "utf8");
  for (const b of src.matchAll(CSS_BLOCK)) selectorClasses(b[1], f, styled, src.slice(0, b.index).split("\n").length);
}

// What the .qwe files can write.
const words = new Set();
const prefixes = new Set();
for (const f of qwe) {
  // Comments name classes without making them (a kit's `size="lg"` in its
  // usage notes): a // at the start of a line or after a space, so that
  // "https://..." stays.
  const src = readFileSync(f, "utf8").replace(CSS_BLOCK, "``").replace(/(^|\s)\/\/.*$/gm, "$1");
  // Markup's shorthand: <div.card.big ...>, <section.lane ...>.
  for (const m of src.matchAll(/<[a-zA-Z][\w-]*((?:\.[a-zA-Z_-][\w-]*)+)/g)) {
    for (const c of m[1].split(".").filter(Boolean)) words.add(c);
  }
  // Every word of every string, those inside a template's ${...} too
  // (`${unread ? "unseen" : ""}`).
  for (const m of src.matchAll(/"((?:[^"\\\n]|\\.)*)"/g)) {
    for (const w of m[1].split(/[^\w-]+/).filter(Boolean)) words.add(w);
  }
  // A template's fixed parts, and the start of a class each ${...} ends:
  // `ui-tabs-${v}`, `p${k}`, `r-${role}`. Only where a class can start: a
  // part right after another ${...} (`${a}r${b}`) is the middle of a word.
  for (const m of src.matchAll(/`([^`]*)`/g)) {
    const parts = m[1].split(/\$\{[^}]*\}/);
    parts.forEach((part, i) => {
      for (const w of part.split(/[^\w-]+/).filter(Boolean)) words.add(w);
      if (i === parts.length - 1) return;
      const tail = part.match(/(?:^|[\s"'])([a-zA-Z][\w-]*)$/);
      if (tail && !(i > 0 && tail.index === 0 && tail[0] === tail[1])) prefixes.add(tail[1]);
    });
  }
}

for (const f of scripts) {
  for (const m of readFileSync(f, "utf8").matchAll(/(["'`])((?:(?!\1)[^\\\n]|\\.)*)\1/g)) {
    for (const w of m[2].split(/[^\w-]+/).filter(Boolean)) words.add(w);
  }
}

// Quillo's own classes, which its runtime sets: ask()'s and notify()'s q-*
// ("q-" + tone gives q-danger and the like), highlight()'s hl-*.
const runtime = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../src/client/runtime.js"), "utf8");
const quillo = new Set([...runtime.matchAll(/"((?:q|hl)-[\w-]+)"/g)].map((m) => m[1]));
const quilloPrefixes = [...runtime.matchAll(/"((?:q|hl)-)" \+/g)].map((m) => m[1]);

// A class a template can make (`ui-avatar-${size}`) is "maybe" when what
// follows the start is named somewhere (size="lg", a default "md"), or is a
// number (`s${k % 8 + 1}`); else no value makes it: unused.
function variant(name) {
  const starts = [...prefixes].filter((p) => name.startsWith(p) && name.length > p.length);
  if (starts.length === 0) return "unused";
  return starts.some((p) => { const rest = name.slice(p.length); return words.has(rest) || /^\d+$/.test(rest); }) ? "maybe" : "unused";
}

const rows = [...styled].map(([name, at]) => {
  const state = words.has(name) ? "used"
    : quillo.has(name) || quilloPrefixes.some((p) => name.startsWith(p)) ? "quillo"
    : variant(name);
  return { name, at, state };
});
const count = (s) => rows.filter((r) => r.state === s).length;
console.log(`${root}: ${rows.length} classes styled in ${sheets.length + qwe.filter((f) => readFileSync(f, "utf8").includes("css`")).length} sheets; ` +
  `${count("used")} written, ${count("quillo")} set by Quillo, ${count("maybe")} maybe (a template's start), ${count("unused")} never written`);
for (const r of rows.sort((a, b) => a.at.localeCompare(b.at, undefined, { numeric: true }))) {
  if (all || r.state === "unused") console.log(`  ${r.state.padEnd(6)} .${r.name.padEnd(28)} ${r.at}`);
}
