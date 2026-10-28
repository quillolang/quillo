// Runs a browser script against a running program and prints what it returns:
//
//   node feedback/tools/drive.mjs my-test.mjs
//
// my-test.mjs default-exports an async function given { browser, page, sleep,
// ALT, CTRL, META, SHIFT } (see chrome.mjs for what a page can do), and
// returns anything JSON can hold. The page's problems and mismatches are
// printed with it.

import { resolve } from "node:path";
import { launch, sleep, ALT, CTRL, META, SHIFT } from "./chrome.mjs";

const file = process.argv[2];
if (!file) {
  console.error("usage: drive.mjs <test.mjs>");
  process.exit(2);
}
const test = (await import(resolve(file))).default;
const browser = await launch({ mismatches: process.argv.includes("--mismatches") });
const page = await browser.page();
let result;
try {
  result = { result: await test({ browser, page, sleep, ALT, CTRL, META, SHIFT }) };
} catch (e) {
  result = { crashed: String(e.stack || e) };
}
console.log(JSON.stringify({ ...result, problems: page.problems, mismatches: page.mismatches }, null, 2));
await browser.close();
process.exit(0);
