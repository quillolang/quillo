// Writes money's locale and symbol tables (LOCALES and SYMBOLS in src/money.rs, MONEY_LOCALES
// and MONEY_SYMBOLS in src/client/runtime.js) from the CLDR data in this Node's ICU, so that
// money.format and format_number write what Intl.NumberFormat writes (FEEDBACK50.md, Q-627;
// branch feedback50-fixes).
//
//   node scripts/money-tables.mjs            # rewrite both files
//   node scripts/money-tables.mjs --check    # say whether they would change, write nothing
//   node scripts/money-tables.mjs --force    # with a CLDR other than 47 (then update
//                                            # tests/money.rs's CLDR, which it checks against)
//
// The locales are the ones listed below. Left out, and written as another with a warning:
// locales whose digits are not 0-9 (ar-EG, fa-IR, bn-BD, mr-IN, my-MM, ne-NP), and those whose
// money the table's four patterns cannot write: right-to-left marks around symbols (he-IL,
// ar-AE, ar-MA), a minus with a mark in plain numbers (ur-PK), and en-150's pattern for each
// currency. tests/money.rs checks the result against Intl for every locale.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CLDR = "47";
if (!(process.versions.cldr ?? "").startsWith(CLDR + ".") && !process.argv.includes("--force")) {
  console.error(`this Node's CLDR is ${process.versions.cldr}, the tables' ${CLDR}: --force to write them from it`);
  process.exit(2);
}

const LOCALES = (
  "en-US en-GB en-CA en-AU en-NZ en-IE en-IN en-SG en-ZA de-DE de-AT de-CH fr-FR fr-BE fr-CA fr-CH it-IT it-CH " +
  "es-ES es-MX es-AR pt-BR pt-PT nl-NL nl-BE sv-SE nb-NO da-DK fi-FI pl-PL cs-CZ hu-HU ro-RO el-GR tr-TR ru-RU uk-UA " +
  "ja-JP zh-CN zh-TW zh-HK ko-KR hi-IN id-ID th-TH vi-VN ms-MY fil-PH " +
  "es-419 es-CO es-CL es-PE es-US es-UY es-VE es-EC es-GT es-CR es-DO es-BO es-PY es-PA es-PR pt-AO pt-MZ " +
  "en-NG en-KE en-PK en-PH en-MY en-AE en-HK en-GH en-IL en-001 fr-MA fr-SN fr-CI fr-LU de-LU de-LI " +
  "hr-HR sl-SI sk-SK bg-BG lt-LT lv-LV et-EE is-IS mk-MK sr-Latn-RS bs-Latn-BA sq-AL ka-GE hy-AM az-Latn-AZ kk-KZ " +
  "uz-Latn-UZ sw-TZ sw-KE zh-SG zh-MO ta-IN te-IN gu-IN kn-IN ml-IN pa-IN km-KH lo-LA si-LK am-ET zu-ZA af-ZA " +
  "ca-ES eu-ES gl-ES ga-IE cy-GB mt-MT lb-LU nn-NO"
).split(" ");

// A locale this ICU does not have would be written as another, quietly.
const missing = LOCALES.filter((l) => Intl.NumberFormat.supportedLocalesOf([l]).length === 0);
if (missing.length) {
  console.error(`this Node's ICU lacks ${missing.join(", ")} (a small-icu build?)`);
  process.exit(2);
}

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const rsFile = path.join(root, "src/money.rs");
const jsFile = path.join(root, "src/client/runtime.js");
let rs = fs.readFileSync(rsFile, "utf8");
let js = fs.readFileSync(jsFile, "utf8");

// The currencies, active and withdrawn, as src/money.rs has them.
const constant = (name) => rs.split(`pub const ${name}: &str = "`)[1].split('";')[0].replace(/\\\n\s*/g, "");
const units = Object.fromEntries((constant("CURRENCIES") + " " + constant("WITHDRAWN")).split(/\s+/).filter(Boolean).map((c) => [c.slice(0, 3), +c.slice(3)]));
const codes = Object.keys(units);

const parts = (l, o, n) => new Intl.NumberFormat(l, o).formatToParts(n);
// A formatted number as a pattern: ¤ the currency, # the number, - the minus sign.
function pattern(ps) {
  let out = "", num = false;
  for (const p of ps) {
    if (["integer", "group", "decimal", "fraction"].includes(p.type)) {
      if (!num) out += "#";
      num = true;
    } else out += p.type === "currency" ? "¤" : p.type === "minusSign" ? "-" : p.value;
  }
  return out;
}
const grouped = (l, o, n) => parts(l, o, n).filter((p) => p.type === "integer" || p.type === "group").map((p) => (p.type === "group" ? "," : p.value)).join("");

const rows = LOCALES.map((l) => {
  const m = parts(l, { style: "currency", currency: "EUR" }, 1234567.5);
  const dec = m.find((p) => p.type === "decimal")?.value ?? ".", grp = m.find((p) => p.type === "group")?.value ?? "";
  const n = parts(l, {}, 1234567.5);
  const ndec = n.find((p) => p.type === "decimal")?.value ?? ".", ngrp = n.find((p) => p.type === "group")?.value ?? "";
  const minus = parts(l, {}, -1).find((p) => p.type === "minusSign").value;
  const flags = (grouped(l, { style: "currency", currency: "EUR" }, 1234567) === "12,34,567" ? 1 : 0)
    | (parts(l, {}, 1234).some((p) => p.type === "group") ? 0 : 2)
    | (grouped(l, {}, 1234567) === "12,34,567" ? 4 : 0);
  const symbolPat = (v) => pattern(parts(l, { style: "currency", currency: "USD", currencyDisplay: "narrowSymbol" }, v));
  const codePat = (v) => pattern(parts(l, { style: "currency", currency: "USD", currencyDisplay: "code" }, v));
  const symbols = Object.fromEntries(codes.map((c) => [c, parts(l, { style: "currency", currency: c }, 1).find((p) => p.type === "currency").value]));
  return { fields: [l, dec, grp, ndec === dec ? "" : ndec, ngrp === grp ? "" : ngrp, minus, String(flags), symbolPat(1), symbolPat(-1), codePat(1), codePat(-1)], symbols };
});

// Each code's symbol in most locales, where that is not the code; each locale's own after.
const base = {};
for (const c of codes) {
  const count = {};
  for (const r of rows) count[r.symbols[c]] = (count[r.symbols[c]] || 0) + 1;
  const [best] = Object.entries(count).sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))[0];
  if (best !== c) base[c] = best;
}
const lines = rows.map((r) => [...r.fields, codes.filter((c) => r.symbols[c] !== (base[c] ?? c)).map((c) => `${c}=${r.symbols[c]}`).join(",")].join("\t"));
const symbols = Object.entries(base).map(([c, s]) => `${c}=${s}`).join(",");

const escape = (s, unicode) => Array.from(s).map((ch) => {
  const o = ch.codePointAt(0);
  if (ch === "\t") return "\\t";
  if (ch === '"' || ch === "\\") return "\\" + ch;
  if (o >= 0x20 && o <= 0x7e) return ch;
  return unicode(o);
}).join("");
const rsEsc = (s) => escape(s, (o) => `\\u{${o.toString(16)}}`);
const jsEsc = (s) => escape(s, (o) => (o < 0x10000 ? [o] : [0xd800 + ((o - 0x10000) >> 10), 0xdc00 + ((o - 0x10000) & 0x3ff)]).map((u) => "\\u" + u.toString(16).padStart(4, "0")).join(""));

function replace(text, start, end, body) {
  const a = text.indexOf(start);
  const b = text.indexOf(end, a) + end.length;
  if (a < 0 || b < end.length) throw new Error(`not found: ${start}`);
  return text.slice(0, a) + body + text.slice(b);
}
// The decimals CLDR writes where they are not the minor units: DISPLAY.
const digits = (c) => new Intl.NumberFormat("en-US", { style: "currency", currency: c }).resolvedOptions().maximumFractionDigits;
const display = codes.filter((c) => digits(c) !== units[c]).map((c) => c + digits(c)).join(" ");

const newRs = [
  ['pub const LOCALES: &str = "', `pub const LOCALES: &str = "${lines.map(rsEsc).join("\\n\\\n")}";\n`],
  ['pub const SYMBOLS: &str = "', `pub const SYMBOLS: &str = "${rsEsc(symbols)}";\n`],
  ['pub const DISPLAY: &str = "', `pub const DISPLAY: &str = "${display}";\n`],
].reduce((t, [start, body]) => replace(t, start, '";\n', body), rs);
const newJs = [
  ['const MONEY_LOCALES = "', `const MONEY_LOCALES = ${lines.map((l, i) => (i ? '  "' : '"') + jsEsc(l) + (i < lines.length - 1 ? "\\n" : "") + '"').join(" +\n")};\n`],
  ['const MONEY_SYMBOLS = "', `const MONEY_SYMBOLS = "${jsEsc(symbols)}";\n`],
  ['const MONEY_DISPLAY = "', `const MONEY_DISPLAY = "${display}";\n`],
].reduce((t, [start, body]) => replace(t, start, '";\n', body), js);

const changed = newRs !== rs || newJs !== js;
console.log(`${lines.length} locales, ${Object.keys(base).length} common symbols; ICU ${process.versions.icu}, CLDR ${process.versions.cldr}`);
if (process.argv.includes("--check")) {
  console.log(changed ? "the tables differ from this ICU's" : "the tables are this ICU's");
  process.exit(changed ? 1 : 0);
}
if (!changed) {
  console.log("nothing to change");
  process.exit(0);
}
// Written beside, then moved over: a file is never left half written.
for (const [file, text] of [[rsFile, newRs], [jsFile, newJs]]) {
  fs.writeFileSync(file + ".tmp", text);
  fs.renameSync(file + ".tmp", file);
}
console.log("rewrote src/money.rs and src/client/runtime.js");
