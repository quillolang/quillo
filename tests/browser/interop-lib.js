// A stand-in for a JavaScript library the page loads (a chart, a map).
window.Gauge = class {
  constructor(el, v) { this.el = el; this.set(v); window.made = (window.made || 0) + 1; }
  set(v) { this.v = v; this.el.textContent = "gauge " + v; }
};
window.lib = {
  double: (x) => x * 2,
  gone: (cls) => { window.gone = (window.gone || []).concat([cls]); },
};
