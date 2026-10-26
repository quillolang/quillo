// The same data and page as quillo.qwe's, for node.js and bun.js.
export const fortunes = [
  { id: 1, message: "fortune: No such file or directory" },
  { id: 2, message: "A computer scientist is someone who fixes things that aren't broken." },
  { id: 3, message: "After enough decimal places, nobody gives a damn." },
  { id: 4, message: "A bad random number generator: 1, 1, 1, 1, 1, 4.33e+67, 1, 1, 1" },
  { id: 5, message: "A computer program does what you tell it to do, not what you want it to do." },
  { id: 6, message: "Emacs is a nice operating system, but I prefer UNIX. — Tom Christaensen" },
  { id: 7, message: "Any program that runs right is obsolete." },
  { id: 8, message: "A list is only as strong as its weakest link. — Donald Knuth" },
  { id: 9, message: "Feature: A bug with seniority." },
  { id: 10, message: "Computers make very fast, very accurate mistakes." },
  { id: 11, message: '<script>alert("This should not be displayed in a browser alert box.");</script>' },
  { id: 12, message: "フレームワークのベンチマーク" },
];
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
export function fortunesPage() {
  const all = [...fortunes, { id: 0, message: "Additional fortune added at request time." }];
  all.sort((a, b) => (a.message < b.message ? -1 : a.message > b.message ? 1 : 0));
  let rows = "";
  for (const f of all) rows += `<tr><td>${f.id}</td><td>${esc(f.message)}</td></tr>`;
  return `<!doctype html><html><head><title>Fortunes</title></head><body><table><tr><th>id</th><th>message</th></tr>${rows}</table></body></html>`;
}
export function work(n) {
  let total = 0;
  for (let i = 0; i < n; i++) total = (total + i * i) % 1000003;
  return total;
}
