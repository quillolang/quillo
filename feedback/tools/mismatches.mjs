// Visits pages of a running Quillo program in headless Chrome and reports
// what fails without a word: components the browser made again because the
// server's HTML did not match (and the text or element that differed),
// errors Quillo caught, exceptions and console errors.
//
//   node feedback/tools/mismatches.mjs http://localhost:8080 / /p/WEB /p/WEB/reports \
//       --login "/login form.stack who=ada password=password123"
//
// --login: a sign-in page, its form's selector, and the fields to fill.
// Exits 1 when any page had a problem.

import { launch, sleep } from "./chrome.mjs";

const args = process.argv.slice(2);
const base = args.shift()?.replace(/\/$/, "");
if (!base) {
  console.error("usage: mismatches.mjs <base url> <path>... [--login \"<path> <form selector> name=value ...\"]");
  process.exit(2);
}
let login = null;
const paths = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--login") login = args[++i].split(/\s+/);
  else paths.push(args[i]);
}

const browser = await launch({ mismatches: true });
const page = await browser.page();
if (login) {
  const [path, selector, ...pairs] = login;
  await page.go(base + path);
  await page.submitForm(selector, Object.fromEntries(pairs.map((p) => p.split("="))));
  page.problems.length = 0;
}
let bad = 0;
for (const path of paths.length ? paths : ["/"]) {
  page.problems.length = 0;
  page.mismatches.length = 0;
  await page.go(base + path);
  await page.booted();
  await sleep(1500);
  const q = await page.quillo();
  const ok = q.remade === 0 && q.errors.length === 0 && page.mismatches.length === 0 && page.problems.length === 0;
  if (!ok) bad++;
  console.log(`${ok ? "ok  " : "FAIL"} ${path}  resumed ${q.resumed}, remade ${q.remade}`);
  for (const m of page.mismatches) console.log(`       mismatch: ${JSON.stringify(m)}`);
  for (const e of q.errors) console.log(`       quillo error: ${e}`);
  for (const p of page.problems) console.log(`       ${p}`);
}
await browser.close();
process.exit(bad ? 1 : 0);
