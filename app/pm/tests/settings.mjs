// A project's settings and reports, in a browser: the columns are a table
// whose rows are components (each keeps what is typed until saved), shown once,
// renamed and moved live; the reports' figures are the same on the server and
// in the browser.
//
//   BASE=http://localhost:8080 node feedback/tools/drive.mjs app/pm/tests/settings.mjs
//
// Needs PM_DEMO=1, freshly seeded.

export default async ({ browser, sleep }) => {
  const base = process.env.BASE;
  const checks = {};
  const check = (name, ok, got) => { checks[name] = ok ? true : (got ?? false); };
  const until = async (p, code, ms = 6000) => {
    for (let t = 0; t < ms; t += 150) { try { if (await p.ev(code)) return true; } catch {} await sleep(150); }
    return false;
  };
  const names = (p) => p.ev(`return [...document.querySelectorAll(".ctable tbody tr input[aria-label=Name]")].map((i) => i.value)`);

  const ada = await browser.page("about:blank", { own: true });
  await ada.go(base + "/login");
  await ada.ev(`const f = document.querySelector("form.stack"); f.who.value = "ada"; f.password.value = "password123"; f.submit(); return true`);
  await sleep(800);
  await ada.go(base + "/p/WEB/settings");
  await ada.booted();
  const before = await names(ada);
  check("the columns are rows of a table", before.length >= 3, before);
  check("each shown once: no row outside the table", await ada.ev(`return document.querySelectorAll("tr").length == document.querySelectorAll("table tr").length`));
  check("taken up as the server sent them", (await ada.quillo()).remade === 0, await ada.quillo());

  await ada.ev(`const i = document.querySelector(".ctable tbody tr input[aria-label=Name]"); i.value = "Ideas"; i.dispatchEvent(new Event("input", { bubbles: true })); return true`);
  check("typing in a row shows its Save", await until(ada, `return [...document.querySelectorAll(".ctable tbody tr")][0].textContent.includes("Save")`));
  await ada.ev(`[...document.querySelectorAll(".ctable tbody tr")][0].querySelector("button.btn.primary").click(); return true`);
  check("saved: the first column is Ideas", await until(ada, `return document.querySelector(".ctable tbody tr input[aria-label=Name]").value == "Ideas" && ![...document.querySelectorAll(".ctable tbody tr")][0].textContent.includes("Save")`), await names(ada));

  await ada.ev(`document.querySelector('.ctable tbody tr button[aria-label="Move right"]').click(); return true`);
  check("moved right: second now", await until(ada, `const n = [...document.querySelectorAll(".ctable tbody tr input[aria-label=Name]")].map((i) => i.value); return n[1] == "Ideas" && n[0] == ${JSON.stringify(before[1])}`), await names(ada));
  check("still one row per column", (await names(ada)).length == before.length, await names(ada));

  await ada.go(base + "/p/WEB/reports");
  await ada.booted();
  const text = await ada.ev(`return document.querySelector("main").innerText`);
  check("the reports show the cycle time", /Cycle time/.test(text), text.slice(0, 300));
  check("no whole number written as N.0", !/\b\d+\.0d\b/.test(text), text.match(/\b\d+\.0d\b/g));
  check("the reports are taken up, not made again", (await ada.quillo()).remade === 0, await ada.quillo());

  return { checks, passed: Object.values(checks).filter((v) => v === true).length, of: Object.keys(checks).length, problems: ada.problems };
};
