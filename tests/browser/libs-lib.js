// A stand-in for a library the page loads by `use` (tests/browser/libs.qwe).
window.Gauge = class {
  constructor(el, v) { this.el = el; this.set(v); window.made = (window.made || 0) + 1; }
  set(v) { this.v = v; this.el.textContent = "gauge " + v; }
};
window.lib = {
  version: "1.2.3",
  double: (x) => x * 2,
  gone: (cls) => { window.gone = (window.gone || []).concat([cls]); },
};
