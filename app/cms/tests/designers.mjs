// The flow and object designers, and an article's blocks, in a browser: the
// transitions and the fields are tables whose rows are components (each keeps
// what is typed until saved), shown once; a transition and a field added, a
// field renamed, a block moved.
//
//   BASE=http://localhost:8080 node feedback/tools/drive.mjs app/cms/tests/designers.mjs
//
// Needs CMS_DEMO=1, freshly seeded.

export default async ({ browser, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const once = (p) => p.ev(`return document.querySelectorAll("tr").length == document.querySelectorAll("table tr").length`);

  const ada = await browser.page("about:blank", { own: true });
  await ada.go(base + "/login");
  await ada.ev(`const f = document.querySelector("form.stack"); f.who.value = "ada"; f.password.value = "password123"; f.submit(); return true`);
  await sleep(800);

  // ---- the flow designer ----
  await ada.go(base + "/desk/flows/1");
  await ada.booted();
  const rows = () => ada.ev(`return document.querySelectorAll(".tr-table tbody tr").length`);
  const n = await rows();
  check("the transitions are rows of a table, the new one last", n >= 2 && await ada.ev(`return document.querySelector(".tr-table tbody tr:last-child").classList.contains("new")`), n);
  check("each shown once", await once(ada));
  check("taken up as the server sent it", (await ada.quillo()).remade === 0, await ada.quillo());
  await ada.ev(`const r = document.querySelector(".tr-table tbody tr.new"); const i = r.querySelector("input[aria-label=Name]"); i.value = "Straight to print"; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  await until(ada, `return !document.querySelector(".tr-table tbody tr.new button.btn.primary").disabled`);
  await ada.ev(`document.querySelector(".tr-table tbody tr.new button.btn.primary").click(); return true`);
  check("a transition added: one row more, and a new empty one", await until(ada, `return document.querySelectorAll(".tr-table tbody tr").length == ${n + 1} && [...document.querySelectorAll(".tr-table tbody tr input[aria-label=Name]")].some((i) => i.value == "Straight to print")`), await rows());

  // ---- the object designer ----
  await ada.go(base + "/desk/objects/1");
  await ada.booted();
  const fields = () => ada.ev(`return [...document.querySelectorAll(".f-table tbody tr input[aria-label='Field name']")].map((i) => i.value)`);
  const before = await fields();
  check("the fields are rows of a table", before.length >= 2, before);
  check("each shown once", await once(ada));
  await ada.ev(`const i = document.querySelector(".f-table tbody tr input[aria-label='Field name']"); i.value = "Client name"; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  await until(ada, `return document.querySelector(".f-table tbody tr button.btn.primary") != null`);
  await ada.ev(`document.querySelector(".f-table tbody tr button.btn.primary").click(); return true`);
  check("a field renamed", await until(ada, `return document.querySelector(".f-table tbody tr input[aria-label='Field name']").value == "Client name" && document.querySelector(".f-table tbody tr button.btn.primary") == null`), await fields());
  await ada.ev(`const f = document.querySelector("form.f-row.new"); const i = f.querySelector("input[aria-label=\\"New field's name\\"]"); i.value = "Notes"; i.dispatchEvent(new Event("input", { bubbles: true })); f.requestSubmit(); return true`);
  check("a field added, as a row of the table", await until(ada, `return [...document.querySelectorAll(".f-table tbody tr input[aria-label='Field name']")].some((i) => i.value == "Notes")`), await fields());
  check("taken up as the server sent it", (await ada.quillo()).remade === 0, await ada.quillo());

  // ---- an article's blocks: the draft (the others are in review, where none move) ----
  await ada.go(base + "/desk/a/3");
  await ada.booted();
  const order = () => ada.ev(`return [...document.querySelectorAll("[id^=block-]")].map((b) => b.id)`);
  const one = (await order()).length;
  await ada.ev(`const adds = document.querySelectorAll(".add-btn"); adds[adds.length - 1].click(); return true`);
  await until(ada, `return document.querySelector(".add-menu button") != null`);
  await ada.ev(`document.querySelector(".add-menu button").click(); return true`);
  check("a block added to the draft", await until(ada, `return document.querySelectorAll("[id^=block-]").length == ${one + 1}`), await order());
  const first = await order();
  await ada.ev(`document.querySelector('button[aria-label="Move down"]').click(); return true`);
  check("a block moved down: the first two change places", await until(ada, `const o = [...document.querySelectorAll("[id^=block-]")].map((b) => b.id); return o[0] == ${JSON.stringify(first[1])} && o[1] == ${JSON.stringify(first[0])}`), await order());

  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems: ada.problems };
};
