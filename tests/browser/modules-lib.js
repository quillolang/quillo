// A stand-in for an ES module the page imports by `use` (tests/browser/modules.qwe).
export const add = (a, b) => a + b;
export const version = "2.0.0";
export class Thing {
  constructor(n) { this.n = n; }
  twice() { return this.n * 2; }
}
export default function hello(name) { return "hello " + name; }
