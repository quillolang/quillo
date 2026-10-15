// Quillo browser runtime: values, the standard library, a small virtual DOM,
// components with ~state, and calls to `server fn`s. The compiled program is
// appended after this and registers its functions in G.
"use strict";

// Where the program is mounted ("" at the root); the bundle sets it.
let BASE = "";
const withBase = (u) => (BASE && typeof u === "string" && u[0] === "/" && u[1] !== "/" ? BASE + u : u);

// ---- values ------------------------------------------------------------

class QuilloError extends Error {}
class Ret { constructor(v) { this.v = v; } }
class Err { constructor(e) { this.error = e === undefined ? null : e; } }
class VNode {}
class VText extends VNode { constructor(t) { super(); this.t = t; } }
class VEl extends VNode { constructor(tag, props, on, kids, key) { super(); this.tag = tag; this.props = props; this.on = on; this.kids = kids; this.key = key; } }
class VComp extends VNode { constructor(def, props, key) { super(); this.def = def; this.props = props; this.key = key; } }
class VRaw extends VNode { constructor(html) { super(); this.html = html; } }

const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
const fail = (msg) => { throw new QuilloError(msg); };
const isNil = (v) => v === null || v === undefined;
const isEvent = (v) => typeof Event !== "undefined" && v instanceof Event;
const isMap = (v) => v !== null && typeof v === "object" && !Array.isArray(v) && !(v instanceof Err) && !(v instanceof VNode) && !isEvent(v);
const isInt = (v) => typeof v === "number" && Number.isInteger(v);

function typeName(v) {
  if (isNil(v)) return "null";
  switch (typeof v) {
    case "boolean": return "bool";
    case "number": return Number.isInteger(v) ? "int" : "float";
    case "string": return "str";
    case "function": return "fn";
  }
  if (Array.isArray(v)) return "list";
  if (v instanceof Err) return "error";
  if (v instanceof VNode) return "html";
  return "map";
}

function typeIs(t, v) {
  switch (t) {
    case "any": return true;
    case "int": return isInt(v);
    // JS numbers do not remember whether they were written 2 or 2.0.
    case "float": return typeof v === "number";
    case "num": return typeof v === "number";
    case "str": return typeof v === "string";
    case "bool": return typeof v === "boolean";
    case "list": return Array.isArray(v);
    case "map": return isMap(v) || isEvent(v);
    case "html": return v instanceof VNode;
    case "fn": return typeof v === "function";
    default: return false;
  }
}

const fmtKey = (k) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(k) ? k : JSON.stringify(k);

function repr(v) {
  if (isNil(v)) return "null";
  if (typeof v === "string") return JSON.stringify(v);
  if (typeof v === "number") return String(v);
  if (typeof v === "boolean") return String(v);
  if (typeof v === "function") return "<fn " + (v.$name || "fn") + ">";
  if (Array.isArray(v)) return "[" + v.map(repr).join(", ") + "]";
  if (v instanceof Err) return "err(" + repr(v.error) + ")";
  if (v instanceof VNode) return "<html>";
  if (isEvent(v)) return "<event " + v.type + ">";
  const ks = Object.keys(v);
  return ks.length ? "{ " + ks.map((k) => fmtKey(k) + ": " + repr(v[k])).join(", ") + " }" : "{}";
}

const display = (v) => typeof v === "string" ? v : repr(v);
const short = (v) => { const r = repr(v); return r.length > 40 ? r.slice(0, 40) + "…" : r; };
const truthy = (v) => !(isNil(v) || v === false || v instanceof Err);

function eq(a, b) {
  if (isNil(a) && isNil(b)) return true;
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => eq(x, b[i]));
  if (a instanceof Err && b instanceof Err) return eq(a.error, b.error);
  if (isMap(a) && isMap(b)) {
    const ka = Object.keys(a);
    return ka.length === Object.keys(b).length && ka.every((k) => hasOwn(b, k) && eq(a[k], b[k]));
  }
  return false;
}

function cmp(a, b) {
  if ((typeof a === "number" && typeof b === "number") || (typeof a === "string" && typeof b === "string")) {
    return a < b ? -1 : a > b ? 1 : 0;
  }
  return null;
}

// ---- operators ---------------------------------------------------------

function add(a, b) {
  if (typeof a === "number" && typeof b === "number") return a + b;
  if (typeof a === "string") return a + display(b);
  if (typeof b === "string") return display(a) + b;
  if (a instanceof VNode && b instanceof VNode) return frag([a, b]);
  if (Array.isArray(a) && Array.isArray(b)) return a.concat(b);
  if (isMap(a) && isMap(b)) return Object.assign({}, a, b);
  fail(`cannot add ${typeName(a)} and ${typeName(b)}`);
}

function arith(op, verb) {
  return (a, b) => {
    if (typeof a !== "number" || typeof b !== "number") fail(`cannot ${verb} ${typeName(a)} and ${typeName(b)}`);
    if (isInt(a) && isInt(b)) {
      if (b === 0 && (op === "/" || op === "%")) fail("division by zero");
    }
    switch (op) {
      case "-": return a - b;
      case "*": return a * b;
      case "/": return a / b;
      default: return a % b;
    }
  };
}
const sub = arith("-", "subtract");
const mul = arith("*", "multiply");
const div = arith("/", "divide");
const mod = arith("%", "take the remainder of");

function order(a, b) {
  const c = cmp(a, b);
  if (c === null) fail(`cannot compare ${typeName(a)} with ${typeName(b)}`);
  return c;
}
const lt = (a, b) => order(a, b) < 0;
const le = (a, b) => order(a, b) <= 0;
const gt = (a, b) => order(a, b) > 0;
const ge = (a, b) => order(a, b) >= 0;

function range(a, b) {
  if (!isInt(a) || !isInt(b)) fail("`..` needs two ints");
  if (b - a > 10000000) fail("range is too large");
  const out = [];
  for (let i = a; i < b; i++) out.push(i);
  return out;
}

function neg(v) {
  if (typeof v !== "number") fail(`cannot negate ${typeName(v)}`);
  return -v;
}

function eventField(e, name) {
  const t = e.target || {};
  switch (name) {
    case "value": return t.value === undefined ? null : t.value;
    case "checked": return !!t.checked;
    case "type": return e.type;
    case "key": return e.key === undefined ? null : e.key;
    case "target": return t;
    case "form": {
      const f = t.tagName === "FORM" ? t : t.form;
      if (!f) return {};
      const out = {};
      for (const [k, v] of new FormData(f)) out[k] = typeof v === "string" ? v : v.name;
      return out;
    }
  }
  // Then the event's own fields (clientX, deltaY, ...), then the element's
  // (scrollTop, clientHeight, dataset, ...).
  for (const src of [e, t]) {
    const v = src[name];
    if (v !== undefined && typeof v !== "function") return v === null ? null : v;
  }
  return null;
}

// `x.name`: a map field, `e.value` on events, or a function called on x.
function field(v, name, fb) {
  if (isNil(v)) return null;
  if (isMap(v)) {
    if (hasOwn(v, name)) return v[name];
    if (name === "len") return B.len(v);
    if (name === "keys") return B.keys(v);
    if (name === "values") return B.values(v);
    return null;
  }
  if (v instanceof Err && name === "error") return v.error;
  if (isEvent(v)) return eventField(v, name);
  const f = fb || (hasOwn(B, name) ? B[name] : null);
  if (typeof f === "function") return f(v);
  fail(`${typeName(v)} has no field \`${name}\``);
}

function index(o, i) {
  if (Array.isArray(o) && isInt(i)) {
    const n = i < 0 ? o.length + i : i;
    return n >= 0 && n < o.length ? o[n] : null;
  }
  if (isMap(o) && typeof i === "string") return hasOwn(o, i) ? o[i] : null;
  if (typeof o === "string" && isInt(i)) {
    const cs = Array.from(o);
    const n = i < 0 ? cs.length + i : i;
    return n >= 0 && n < cs.length ? cs[n] : null;
  }
  if (isNil(o)) return null;
  fail(`cannot index ${typeName(o)} with ${typeName(i)}`);
}

function call(f, args) {
  if (typeof f !== "function") fail(`cannot call ${typeName(f)}: it is not a function`);
  return f(...args);
}

// `recv.name(args)`: a function stored in the map, or `name(recv, args)`.
function mcall(recv, name, fb, args) {
  if (isMap(recv) && hasOwn(recv, name)) return call(recv[name], args);
  if (typeof fb === "function") return fb(recv, ...args);
  fail(`${typeName(recv)} has no method \`${name}\``);
}

function tryv(v) {
  if (v instanceof Err) throw new Ret(v);
  return v;
}

function iter(v) {
  if (Array.isArray(v)) return v;
  if (isMap(v)) return Object.keys(v).map((k) => [k, v[k]]);
  if (typeof v === "string") return Array.from(v);
  if (isNil(v)) return [];
  fail(`cannot loop over ${typeName(v)}`);
}

function tyText(names, opt) {
  return names.join(" | ") + (opt ? (names.length === 1 ? "?" : " | null") : "");
}

function tyOk(v, names, opt) {
  return (opt && isNil(v)) || names.some((t) => typeIs(t, v));
}

// Argument count and type checks, as on the server.
function arity(n, max, name) {
  if (n > max) fail(`\`${name}\` takes ${max} argument${max === 1 ? "" : "s"}, got ${n}`);
}
function missing(p, name) { fail(`missing argument \`${p}\` in call to \`${name}\``); }
function argty(v, names, opt, p, name) {
  if (!tyOk(v, names, opt)) fail(`argument \`${p}\` of \`${name}\` must be ${tyText(names, opt)}, got ${typeName(v)} ${short(v)}`);
}
function retty(v, names, opt, name, enames, eopt) {
  if (v instanceof Err) {
    if (enames && !tyOk(v.error, enames, eopt)) fail(`\`${name}\` declares errors as \`! ${tyText(enames, eopt)}\`, but returned err(${short(v.error)})`);
    return v;
  }
  if (names && !tyOk(v, names, opt)) fail(`\`${name}\` must return ${tyText(names, opt)}, but returned ${typeName(v)} ${short(v)}`);
  return v;
}

function meta(f, name, params) {
  f.$name = name;
  f.$p = params;
  return f;
}

// ---- patterns ----------------------------------------------------------
// { k: "w" } wild, { k: "b", n } bind, { k: "l", v } literal, { k: "t", t } type,
// { k: "a", ps } alternatives, { k: "as", p, n }, { k: "ls", ps, rest },
// { k: "r", fs: [{ path, p, d }] } record (d: a default thunk or null).

function getPath(v, path) {
  let cur = v;
  for (const k of path) {
    if (!isMap(cur) || !hasOwn(cur, k)) return undefined;
    cur = cur[k];
  }
  return cur;
}

function matchInto(p, v, b) {
  switch (p.k) {
    case "w": return true;
    case "b": b[p.n] = v; return true;
    case "l": return eq(p.v, v);
    case "t": return typeIs(p.t, v);
    case "a": {
      for (const q of p.ps) {
        const t = {};
        if (matchInto(q, v, t)) { Object.assign(b, t); return true; }
      }
      return false;
    }
    case "as": {
      if (!matchInto(p.p, v, b)) return false;
      b[p.n] = v;
      return true;
    }
    case "ls": {
      if (!Array.isArray(v)) return false;
      if (v.length < p.ps.length || (p.rest === undefined && v.length !== p.ps.length)) return false;
      for (let i = 0; i < p.ps.length; i++) if (!matchInto(p.ps[i], v[i], b)) return false;
      if (p.rest) b[p.rest] = v.slice(p.ps.length);
      return true;
    }
    case "r": {
      if (!isMap(v)) return false;
      for (const f of p.fs) {
        let x = getPath(v, f.path);
        if (isNil(x)) {
          if (f.d) {
            x = f.d();
            // A default is taken as written, even `int = null`.
            if (f.p.k === "t") { b[f.path[f.path.length - 1]] = x; continue; }
          } else if (x === undefined) return false;
          else x = null;
        }
        const before = Object.keys(b).length;
        if (!matchInto(f.p, x, b)) return false;
        if (f.p.k === "t" && Object.keys(b).length === before) b[f.path[f.path.length - 1]] = x;
      }
      return true;
    }
  }
  return false;
}

function match(p, v) {
  const b = {};
  return matchInto(p, v, b) ? b : null;
}

// ---- the standard library (the parts that make sense in a browser) -----

const want = (ok, f, what, i, v) => { if (!ok) fail(`\`${f}\` expects ${what} as argument ${i}, got ${isNil(v) && v === undefined ? "nothing" : typeName(v)}`); };
const S = (v, f, i = 1) => { want(typeof v === "string", f, "a str", i, v); return v; };
const L = (v, f, i = 1) => { if (isNil(v)) return []; want(Array.isArray(v), f, "a list", i, v); return v; };
const I = (v, f, i = 1) => { want(isInt(v), f, "an int", i, v); return v; };
const M = (v, f, i = 1) => { if (isNil(v)) return {}; want(isMap(v), f, "a map", i, v); return v; };
const F = (v, f, i = 2) => { want(typeof v === "function", f, "a function", i, v); return v; };
const N = (v, f, i = 1) => { want(typeof v === "number", f, "a number", i, v); return v; };

// Calls a function given to a builtin; it must not wait on the server.
function cb(f, who, ...args) {
  const r = f(...args);
  if (r instanceof Promise) fail(`\`${who}\` cannot call a function that waits on the server; use a \`for\` loop instead`);
  return r;
}

function clampRange(len, start, end) {
  const fix = (i) => Math.min(Math.max(i < 0 ? len + i : i, 0), len);
  const s = fix(start);
  const e = end === undefined ? len : fix(end);
  return [s, Math.max(e, s)];
}

function sortValues(items, name) {
  let bad = null;
  const sorted = items.slice().sort((a, b) => {
    const c = cmp(a[0], b[0]);
    if (c === null) { bad = [typeName(a[0]), typeName(b[0])]; return 0; }
    return c;
  });
  if (bad) fail(`\`${name}\` cannot compare ${bad[0]} with ${bad[1]}`);
  return sorted.map((x) => x[1]);
}

function extreme(l, name, want) {
  let best = null;
  for (const x of L(l, name)) {
    if (best === null) { best = x; continue; }
    const c = cmp(x, best);
    if (c === null) fail(`\`${name}\` cannot compare ${typeName(x)} with ${typeName(best)}`);
    if (c === want) best = x;
  }
  return best;
}

function coerce(t, s) {
  s = s.trim();
  if (t === "int") return /^[+-]?\d+$/.test(s) ? parseInt(s, 10) : null;
  if (t === "float" || t === "num") return s !== "" && !isNaN(Number(s)) ? Number(s) : null;
  return null;
}

function toJSON(v) {
  if (isNil(v)) return null;
  if (v instanceof Err) return { error: toJSON(v.error) };
  if (Array.isArray(v)) return v.map(toJSON);
  if (v instanceof VNode) return "";
  if (typeof v === "function") return repr(v);
  if (isMap(v)) { const o = {}; for (const k of Object.keys(v)) o[k] = toJSON(v[k]); return o; }
  if (typeof v === "number" && !isFinite(v)) return null;
  return v;
}

const B = {};
B.print = meta((...a) => { console.log(a.map(display).join(" ")); return null; }, "print", null);
B.str = meta((v) => display(v), "str", null);
B.int = meta((v) => {
  if (isInt(v)) return v;
  if (typeof v === "number" && isFinite(v)) return Math.trunc(v);
  if (typeof v === "boolean") return v ? 1 : 0;
  if (typeof v === "string") { const n = coerce("int", v); return n === null ? new Err(`not an int: ${JSON.stringify(v)}`) : n; }
  return new Err(`cannot convert ${typeName(v)} to int`);
}, "int", null);
B.float = meta((v) => {
  if (typeof v === "number") return v;
  if (typeof v === "string") { const n = coerce("float", v); return n === null ? new Err(`not a number: ${JSON.stringify(v)}`) : n; }
  return new Err(`cannot convert ${typeName(v)} to float`);
}, "float", null);
B.len = meta((v) => {
  if (typeof v === "string") return Array.from(v).length;
  if (Array.isArray(v)) return v.length;
  if (isMap(v)) return Object.keys(v).length;
  if (isNil(v)) return 0;
  fail(`\`len\` expects a str, list or map, got ${typeName(v)}`);
}, "len", null);
B.type_of = meta((v) => typeName(v), "type_of", null);
B.upper = meta((s) => S(s, "upper").toUpperCase(), "upper", null);
B.lower = meta((s) => S(s, "lower").toLowerCase(), "lower", null);
B.trim = meta((s) => S(s, "trim").trim(), "trim", null);
B.split = meta((s, sep = " ") => { S(s, "split"); S(sep, "split", 2); return sep === "" ? Array.from(s) : s.split(sep); }, "split", null);
B.join = meta((l, sep = "") => L(l, "join").map(display).join(S(sep, "join", 2)), "join", null);
B.contains = meta((c, x) => {
  if (typeof c === "string") return c.includes(display(x));
  if (Array.isArray(c)) return c.some((y) => eq(y, x));
  if (isMap(c)) return hasOwn(c, display(x));
  if (isNil(c)) return false;
  fail(`\`contains\` expects a str, list or map, got ${typeName(c)}`);
}, "contains", null);
B.starts_with = meta((s, p) => S(s, "starts_with").startsWith(S(p, "starts_with", 2)), "starts_with", null);
B.ends_with = meta((s, p) => S(s, "ends_with").endsWith(S(p, "ends_with", 2)), "ends_with", null);
B.replace = meta((s, a, b) => S(s, "replace").split(S(a, "replace", 2)).join(S(b, "replace", 3)), "replace", null);
B.slice = meta((v, start, end) => {
  I(start, "slice", 2);
  if (end !== undefined) I(end, "slice", 3);
  if (Array.isArray(v)) { const [s, e] = clampRange(v.length, start, end); return v.slice(s, e); }
  if (typeof v === "string") { const cs = Array.from(v); const [s, e] = clampRange(cs.length, start, end); return cs.slice(s, e).join(""); }
  fail(`\`slice\` expects a list or str, got ${typeName(v)}`);
}, "slice", null);
B.map = meta((l, f) => { F(f, "map"); return L(l, "map").map((x) => cb(f, "map", x)); }, "map", null);
B.filter = meta((l, f) => { F(f, "filter"); return L(l, "filter").filter((x) => truthy(cb(f, "filter", x))); }, "filter", null);
B.find = meta((l, f) => { F(f, "find"); for (const x of L(l, "find")) if (truthy(cb(f, "find", x))) return x; return null; }, "find", null);
B.any = meta((l, f) => { F(f, "any"); return L(l, "any").some((x) => truthy(cb(f, "any", x))); }, "any", null);
B.all = meta((l, f) => { F(f, "all"); return L(l, "all").every((x) => truthy(cb(f, "all", x))); }, "all", null);
B.reduce = meta((l, acc, f) => { F(f, "reduce", 3); for (const x of L(l, "reduce")) acc = cb(f, "reduce", acc, x); return acc; }, "reduce", null);
B.sum = meta((l) => L(l, "sum").reduce((a, x) => add(a, x), 0), "sum", null);
B.min = meta((l) => extreme(l, "min", -1), "min", null);
B.max = meta((l) => extreme(l, "max", 1), "max", null);
B.sort = meta((l) => sortValues(L(l, "sort").map((v) => [v, v]), "sort"), "sort", null);
B.sort_by = meta((l, f) => { F(f, "sort_by"); return sortValues(L(l, "sort_by").map((v) => [cb(f, "sort_by", v), v]), "sort_by"); }, "sort_by", null);
B.reverse = meta((v) => {
  if (Array.isArray(v)) return v.slice().reverse();
  if (typeof v === "string") return Array.from(v).reverse().join("");
  fail(`\`reverse\` expects a list or str, got ${typeName(v)}`);
}, "reverse", null);
B.first = meta((l) => { l = L(l, "first"); return l.length ? l[0] : null; }, "first", null);
B.last = meta((l) => { l = L(l, "last"); return l.length ? l[l.length - 1] : null; }, "last", null);
B.push = meta((l, x = null) => L(l, "push").concat([x]), "push", null);
B.range = meta((a, b) => b === undefined ? range(0, I(a, "range")) : range(I(a, "range"), I(b, "range", 2)), "range", null);
B.enumerate = meta((l) => L(l, "enumerate").map((v, i) => [i, v]), "enumerate", null);
B.keys = meta((m) => Object.keys(M(m, "keys")), "keys", null);
B.values = meta((m) => { m = M(m, "values"); return Object.keys(m).map((k) => m[k]); }, "values", null);
B.get = meta((m, k, d = null) => { m = M(m, "get"); k = display(k); return hasOwn(m, k) ? m[k] : d; }, "get", null);
B.has = meta((m, k) => hasOwn(M(m, "has"), display(k)), "has", null);
B.set = meta((m, k, v = null) => Object.assign({}, M(m, "set"), { [display(k)]: v }), "set", null);
B.remove = meta((c, k) => {
  if (Array.isArray(c)) return c.filter((v) => !eq(v, k));
  const m = Object.assign({}, M(c, "remove"));
  delete m[display(k)];
  return m;
}, "remove", null);
B.merge = meta((a, b) => Object.assign({}, M(a, "merge"), M(b, "merge", 2)), "merge", null);
B.err = meta((v = null) => new Err(v), "err", null);
B.is_err = meta((v) => v instanceof Err, "is_err", null);
B.unwrap = meta((v) => { if (v instanceof Err) fail(`unwrap of an error: ${display(v.error)}`); return v; }, "unwrap", null);
B.raw = meta((v) => new VRaw(display(v)), "raw", null);
B.escape = meta((v) => display(v).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]), "escape", null);
B.now = meta(() => Date.now(), "now", null);
B.random = meta(() => Math.random(), "random", null);
B.random_int = meta((lo, hi) => {
  I(lo, "random_int"); I(hi, "random_int", 2);
  if (lo > hi) fail(`\`random_int\` needs lo <= hi, got ${lo} and ${hi}`);
  return lo + Math.floor(Math.random() * (hi - lo + 1));
}, "random_int", null);
B.uuid = meta(() => (typeof crypto !== "undefined" && crypto.randomUUID) ? crypto.randomUUID()
  : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => { const r = Math.random() * 16 | 0; return (c === "x" ? r : (r & 3 | 8)).toString(16); }), "uuid", null);
B.abs = meta((x) => Math.abs(N(x, "abs")), "abs", null);
B.round = meta((x, d) => { N(x, "round"); if (d === undefined) return Math.round(x); const p = 10 ** Math.min(Math.max(I(d, "round", 2), 0), 15); return Math.round(x * p) / p; }, "round", null);
B.floor = meta((x) => Math.floor(N(x, "floor")), "floor", null);
B.ceil = meta((x) => Math.ceil(N(x, "ceil")), "ceil", null);
B.sqrt = meta((x) => Math.sqrt(N(x, "sqrt")), "sqrt", null);
B.pow = meta((x, y) => N(x, "pow") ** N(y, "pow", 2), "pow", null);
B.unique = meta((l) => { const out = []; for (const x of L(l, "unique")) if (!out.some((y) => eq(x, y))) out.push(x); return out; }, "unique", null);
B.flatten = meta((l) => [].concat(...L(l, "flatten").map((x) => Array.isArray(x) ? x : [x])), "flatten", null);
B.zip = meta((a, b) => { a = L(a, "zip"); b = L(b, "zip", 2); return a.slice(0, Math.min(a.length, b.length)).map((x, i) => [x, b[i]]); }, "zip", null);
B.take = meta((v, n) => { n = Math.max(I(n, "take", 2), 0); return typeof v === "string" ? Array.from(v).slice(0, n).join("") : L(v, "take").slice(0, n); }, "take", null);
B.drop = meta((v, n) => { n = Math.max(I(n, "drop", 2), 0); return typeof v === "string" ? Array.from(v).slice(n).join("") : L(v, "drop").slice(n); }, "drop", null);
B.count = meta((l, f) => { l = L(l, "count"); if (f === undefined) return l.length; F(f, "count"); return l.filter((x) => truthy(cb(f, "count", x))).length; }, "count", null);
B.group_by = meta((l, f) => { F(f, "group_by"); const out = {}; for (const x of L(l, "group_by")) { const k = display(cb(f, "group_by", x)); (out[k] = out[k] || []).push(x); } return out; }, "group_by", null);
B.index_of = meta((c, x) => {
  if (Array.isArray(c)) { const i = c.findIndex((y) => eq(y, x)); return i < 0 ? null : i; }
  if (typeof c === "string") { const i = c.indexOf(display(x)); return i < 0 ? null : Array.from(c.slice(0, i)).length; }
  fail(`\`index_of\` expects a list or str, got ${typeName(c)}`);
}, "index_of", null);
B.repeat = meta((s, n) => { S(s, "repeat"); I(n, "repeat", 2); if (n < 0 || s.length * n > 1e8) fail("`repeat` count is negative or too large"); return s.repeat(n); }, "repeat", null);
B.url = meta(() => currentUrl(), "url", null);
B.navigate = meta((to) => navigate(S(to, "navigate")), "navigate", null);
B.json = {
  parse: (s) => { S(s, "json.parse"); try { return fromJSON(JSON.parse(s)); } catch (e) { return new Err("invalid JSON: " + e.message); } },
  stringify: (v, pretty) => JSON.stringify(toJSON(v), null, truthy(pretty) ? 2 : undefined),
};

function fromJSON(j) {
  if (j === undefined) return null;
  return j;
}

// ---- server fns --------------------------------------------------------

async function rpcRaw(name, args, live) {
  let r;
  const headers = { "content-type": "application/json" };
  if (live) headers["x-quillo-live"] = "1";
  // A call that fails (no connection, a server error) is an error value,
  // as with http.* on the server, so `??` and `is_err` handle it.
  const failed = (msg) => ({ value: new Err(msg), topics: [], seq: 0, failed: true });
  try {
    r = await fetch(BASE + "/_quillo/rpc/" + encodeURIComponent(name), {
      method: "POST",
      headers,
      body: JSON.stringify(args.map(toJSON)),
    });
  } catch (e) {
    return failed(`server fn \`${name}\` could not be reached: ${e.message}`);
  }
  let body = null;
  try { body = await r.json(); } catch (_) {}
  if (!r.ok) return failed(`server fn \`${name}\` failed (${r.status})${body && body.error ? ": " + body.error : ""}`);
  const value = body && hasOwn(body, "err") ? new Err(body.err) : body && hasOwn(body, "ok") ? body.ok : null;
  return { value, topics: (body && body.topics) || [], seq: (body && body.seq) || 0 };
}

async function rpc(name, args) {
  return (await rpcRaw(name, args, false)).value;
}

// ---- live updates --------------------------------------------------------
// Each `~x = live(load, ...)` is a subscription to the topics `load` read
// (tables and globals). One request at a time waits at /_quillo/live for any
// of them to change; then the affected loaders run again.

const Live = { seq: 0, subs: new Set(), ctrl: null, running: false, key: "" };

function liveSeen(seq) {
  if (seq > 0) Live.seq = Live.seq > 0 ? Math.min(Live.seq, seq) : seq;
}

function liveTopics() {
  const all = new Set();
  for (const s of Live.subs) {
    if (s.inst.kids !== null && !s.inst.el.isConnected) { Live.subs.delete(s); continue; }
    for (const t of s.topics) all.add(t);
  }
  return [...all].sort();
}

// Starts waiting, or starts again when the set of topics changed.
function liveRestart() {
  queueMicrotask(() => {
    if (!Live.running) {
      Live.running = true;
      liveLoop();
    } else if (liveTopics().join(",") !== Live.key && Live.ctrl) {
      Live.ctrl.abort();
    }
  });
}

async function liveLoop() {
  for (;;) {
    const topics = liveTopics();
    Live.key = topics.join(",");
    if (!topics.length) { Live.running = false; return; }
    const ctrl = (Live.ctrl = new AbortController());
    let j;
    try {
      const r = await fetch(BASE + "/_quillo/live?since=" + Live.seq + "&t=" + encodeURIComponent(Live.key), { signal: ctrl.signal });
      j = await r.json();
    } catch (e) {
      if (!ctrl.signal.aborted) await new Promise((r) => setTimeout(r, 1000));
      continue;
    }
    Live.seq = j.seq;
    const changed = new Set(j.changed || []);
    if (changed.size) for (const s of [...Live.subs]) if (s.topics.some((t) => changed.has(t))) liveRefresh(s);
  }
}

async function liveRefresh(sub) {
  try {
    const { value, topics, seq, failed } = await rpcRaw(sub.fname, sub.args, true);
    // Keep showing the last good data; the next change tries again.
    if (failed) { report(value.error); return; }
    liveSeen(seq);
    sub.topics = topics;
    sub.inst.set(sub.name, value);
    liveRestart();
  } catch (e) {
    report(e);
  }
}

// ---- virtual DOM -------------------------------------------------------

function frag(items) { return items; }

function norm(x, out) {
  if (isNil(x) || x === false || x === true) return out;
  if (Array.isArray(x)) { for (const y of x) norm(y, out); return out; }
  if (x instanceof VNode) { out.push(x); return out; }
  out.push(new VText(display(x)));
  return out;
}

// HTML literal: W.h(tag, props, on, kids, key)
function h(tag, props, on, kids, key) {
  return new VEl(tag, props, on, norm(kids, []), isNil(key) ? null : display(key));
}

// `<Card .../>`: a view call, or a component instance. Spread maps fill
// the parameters not written out; their other keys are ignored.
function comp(def, props, kids, key, spreads) {
  if (typeof def !== "function") fail(`<${def}> is not a view`);
  const ps = def.$p || [];
  for (const k of Object.keys(props)) if (!ps.includes(k)) fail(`<${def.$name}> has no parameter \`${k}\``);
  for (const m of spreads || []) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...}\` in <${def.$name}> needs a map, got ${typeName(m)}`);
    for (const k of Object.keys(m)) if (ps.includes(k) && k !== "children" && k !== "key" && !hasOwn(props, k)) props[k] = m[k];
  }
  if (ps.includes("children")) props.children = norm(kids, []);
  if (def.$comp) return new VComp(def, props, isNil(key) ? null : display(key));
  return def(...ps.map((p) => props[p]));
}

// An element with `{...map}` attributes: written-out ones win, classes add up,
// and `@event` keys become handlers.
function hs(tag, spreads, props, on, kids, key) {
  const p = {};
  const o = Object.assign({}, on || {});
  const classes = [];
  for (const m of spreads) {
    if (isNil(m)) continue;
    if (!isMap(m)) fail(`\`{...}\` in <${tag}> needs a map, got ${typeName(m)}`);
    for (const k of Object.keys(m)) {
      if (k === "key") continue;
      if (k === "class") { classes.push(m[k]); continue; }
      if (k[0] === "@") { if (!hasOwn(o, k.slice(1))) o[k.slice(1)] = m[k]; continue; }
      if (!hasOwn(props, k)) p[k] = m[k];
    }
  }
  Object.assign(p, props);
  if (classes.length) p.class = [props.class, ...classes].filter((x) => !isNil(x) && x !== false && x !== "").map(display).join(" ");
  return h(tag, p, Object.keys(o).length ? o : null, kids, key);
}

// ---- client-side navigation ---------------------------------------------
// Components that call url() re-render when the URL changes. Once one has,
// clicks on same-site links inside components change the URL without a
// reload (not with a modifier key, target, download or data-reload).

let rendering = null;
const Nav = { subs: new Set(), on: false };

function navChanged() {
  for (const i of [...Nav.subs]) {
    if (i.kids !== null && !i.el.isConnected) { Nav.subs.delete(i); continue; }
    i.update();
  }
}

function navStart() {
  if (Nav.on || typeof window === "undefined") return;
  Nav.on = true;
  window.addEventListener("popstate", navChanged);
  document.addEventListener("click", (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a || !a.closest("quillo-island")) return;
    if ((a.target && a.target !== "_self") || a.hasAttribute("download") || a.hasAttribute("data-reload")) return;
    const u = new URL(a.href, location.href);
    if (u.origin !== location.origin) return;
    // Only links into this program; others load normally.
    if (BASE && u.pathname !== BASE && !u.pathname.startsWith(BASE + "/")) return;
    const path = u.pathname.slice(BASE.length) || "/";
    if (path.startsWith("/_quillo/")) return;
    e.preventDefault();
    navigate(path + u.search + u.hash);
  });
}

function navigate(to) {
  navStart();
  const full = withBase(to);
  if (full !== location.pathname + location.search + location.hash) history.pushState(null, "", full);
  window.scrollTo(0, 0);
  navChanged();
  return null;
}

function currentUrl() {
  navStart();
  if (rendering) Nav.subs.add(rendering);
  const p = location.pathname;
  return {
    path: BASE && p.startsWith(BASE) ? p.slice(BASE.length) || "/" : p,
    query: Object.fromEntries(new URLSearchParams(location.search)),
    hash: location.hash.slice(1),
  };
}

const SVG = "http://www.w3.org/2000/svg";
let hydrating = null;

function setProp(el, k, v, old) {
  if (k === "value" || k === "checked" || k === "selected") {
    const nv = k === "value" ? (isNil(v) ? "" : display(v)) : truthy(v);
    if (el[k] !== nv) el[k] = nv;
    if (k !== "value") { if (nv) el.setAttribute(k, ""); else el.removeAttribute(k); }
    return;
  }
  if (isNil(v) || v === false) { el.removeAttribute(k); return; }
  if (k === "href" || k === "src" || k === "action" || k === "formaction" || k === "poster") v = withBase(v);
  const s = v === true ? "" : (Array.isArray(v) || isMap(v)) ? JSON.stringify(toJSON(v)) : display(v);
  if (old !== v || el.getAttribute(k) !== s) el.setAttribute(k, s);
}

function fire(el, type, ev) {
  const f = el.__w && el.__w[type];
  if (typeof f !== "function") return;
  if (type === "submit") ev.preventDefault();
  try {
    const r = f.length > 0 ? f(ev) : f();
    if (r instanceof Promise) r.catch(report);
  } catch (e) {
    report(e);
  }
}

function setEvents(el, on) {
  const cur = el.__w || (el.__w = {});
  for (const t of Object.keys(cur)) if (!on || !hasOwn(on, t)) cur[t] = null;
  if (!on) return;
  const listening = el.__wl || (el.__wl = {});
  for (const t of Object.keys(on)) {
    if (!listening[t]) {
      listening[t] = true;
      el.addEventListener(t, (ev) => fire(el, t, ev));
    }
    cur[t] = on[t];
  }
}

function mount(v, svg) {
  if (v instanceof VText) return (v.d = document.createTextNode(v.t));
  if (v instanceof VRaw) {
    const el = document.createElement("quillo-raw");
    el.style.display = "contents";
    el.innerHTML = v.html;
    return (v.d = el);
  }
  if (v instanceof VComp) {
    const h = takeHydration(v.def.$name);
    const inst = new Inst(v.def, v.props, h && h.s, h && h.l);
    const el = document.createElement("quillo-island");
    el.style.display = "contents";
    inst.el = el;
    inst.update();
    v.inst = inst;
    return (v.d = el);
  }
  const isSvg = svg || v.tag === "svg";
  const el = isSvg ? document.createElementNS(SVG, v.tag) : document.createElement(v.tag);
  for (const k of Object.keys(v.props)) setProp(el, k, v.props[k], undefined);
  setEvents(el, v.on);
  for (const k of v.kids) el.appendChild(mount(k, isSvg && v.tag !== "foreignObject"));
  return (v.d = el);
}

function same(a, b) {
  if (a.constructor !== b.constructor) return false;
  if (a instanceof VEl) return a.tag === b.tag && a.key === b.key;
  if (a instanceof VComp) return a.def === b.def && a.key === b.key;
  return true;
}

function patch(parent, a, b, svg) {
  if (!same(a, b)) {
    const d = mount(b, svg);
    parent.replaceChild(d, a.d);
    return;
  }
  const d = (b.d = a.d);
  if (b instanceof VText) { if (a.t !== b.t) d.nodeValue = b.t; return; }
  if (b instanceof VRaw) { if (a.html !== b.html) d.innerHTML = b.html; return; }
  if (b instanceof VComp) {
    b.inst = a.inst;
    b.inst.props = b.props;
    b.inst.update();
    return;
  }
  const isSvg = svg || b.tag === "svg";
  for (const k of Object.keys(a.props)) if (!hasOwn(b.props, k)) setProp(d, k, null, a.props[k]);
  for (const k of Object.keys(b.props)) setProp(d, k, b.props[k], a.props[k]);
  setEvents(d, b.on);
  patchKids(d, a.kids, b.kids, isSvg && b.tag !== "foreignObject");
}

function keyOf(v) { return v instanceof VEl || v instanceof VComp ? v.key : null; }

function patchKids(el, old, kids, svg) {
  if (kids.some((k) => keyOf(k) !== null)) return patchKeyed(el, old, kids, svg);
  const n = Math.min(old.length, kids.length);
  for (let i = 0; i < n; i++) patch(el, old[i], kids[i], svg);
  for (let i = n; i < kids.length; i++) el.appendChild(mount(kids[i], svg));
  for (let i = n; i < old.length; i++) unmount(el, old[i]);
}

function patchKeyed(el, old, kids, svg) {
  const byKey = new Map();
  const unkeyed = [];
  for (const o of old) { const k = keyOf(o); if (k === null) unkeyed.push(o); else byKey.set(k, o); }
  let u = 0;
  let anchor = old.length ? old[0].d : null;
  const used = new Set();
  for (const k of kids) {
    const key = keyOf(k);
    let o = key === null ? unkeyed[u++] : byKey.get(key);
    if (o && !same(o, k)) o = null;
    if (o) {
      used.add(o);
      patch(el, o, k, svg);
    } else {
      mount(k, svg);
    }
    if (k.d !== anchor) el.insertBefore(k.d, anchor);
    else anchor = anchor.nextSibling;
  }
  for (const o of old) if (!used.has(o) && o.d && o.d.parentNode === el) el.removeChild(o.d);
}

function unmount(el, v) {
  if (v.d && v.d.parentNode === el) el.removeChild(v.d);
}

// ---- components --------------------------------------------------------

class Inst {
  constructor(def, props, state, live) {
    this.def = def;
    this.props = props;
    this.liveInfo = live || null;
    this.subs = {};
    this.s = state || {};
    this.ready = new Set(Object.keys(this.s));
    this.kids = null;
    this.el = null;
    this.queued = false;
  }

  // `~name = init`: runs init the first time only.
  state(name, init) {
    if (this.ready.has(name)) return;
    this.ready.add(name);
    const v = init();
    if (v instanceof Promise) {
      this.s[name] = null;
      v.then((x) => this.set(name, x), report);
    } else {
      this.s[name] = v;
    }
  }

  set(name, v) {
    this.s[name] = v;
    if (!this.queued) {
      this.queued = true;
      queueMicrotask(() => { this.queued = false; this.update(); });
    }
    return null;
  }

  render() {
    const ps = this.def.$p;
    const args = ps.map((p) => {
      const v = this.props[p];
      return p === "children" && typeof v === "string" ? new VRaw(v) : v;
    });
    const outer = rendering;
    rendering = this;
    try {
      return norm(this.def(this, ...args), []);
    } catch (e) {
      if (e instanceof Ret) return norm(e.v, []);
      throw e;
    } finally {
      rendering = outer;
    }
  }

  update() {
    let kids;
    try {
      kids = this.render();
    } catch (e) {
      report(e);
      return;
    }
    if (this.kids === null) {
      this.el.replaceChildren(...kids.map((k) => mount(k, false)));
    } else {
      patchKids(this.el, this.kids, kids, false);
    }
    this.kids = kids;
  }
}

// Island JSON, with error values (`{"$quillo_err": x}`) restored.
function islandJSON(text, fallback) {
  if (!text) return fallback;
  return JSON.parse(text, (k, v) => (v && typeof v === "object" && !Array.isArray(v) && hasOwn(v, "$quillo_err") && Object.keys(v).length === 1) ? new Err(v.$quillo_err) : v);
}

function takeHydration(name) {
  const q = hydrating && hydrating[name];
  return q && q.length ? q.shift() : null;
}

function report(e) {
  console.error("quillo:", e instanceof Error ? e.message : e);
  if (typeof window !== "undefined" && window.__quilloErrors) window.__quilloErrors.push(String(e && e.message || e));
}

function boot() {
  for (const island of document.querySelectorAll("quillo-island")) {
    if (island.parentElement && island.parentElement.closest("quillo-island")) continue;
    const def = C[island.dataset.c];
    if (!def) { report(`unknown component <${island.dataset.c}>`); continue; }
    hydrating = {};
    for (const inner of island.querySelectorAll("quillo-island")) {
      (hydrating[inner.dataset.c] = hydrating[inner.dataset.c] || []).push({
        s: islandJSON(inner.dataset.s, {}),
        l: islandJSON(inner.dataset.l, null),
      });
      liveSeen(+inner.dataset.v || 0);
    }
    liveSeen(+island.dataset.v || 0);
    const inst = new Inst(def, islandJSON(island.dataset.p, {}), islandJSON(island.dataset.s, {}), islandJSON(island.dataset.l, null));
    inst.el = island;
    inst.update();
    hydrating = null;
  }
}

// ---- what compiled code uses -------------------------------------------

const G = {};
const C = {};
const W = {};
W.Err = Err;
W.Ret = Ret;
W.fail = fail;
W.truthy = truthy;
W.eq = eq;
W.add = add;
W.sub = sub;
W.mul = mul;
W.div = div;
W.mod = mod;
W.lt = lt;
W.le = le;
W.gt = gt;
W.ge = ge;
W.range = range;
W.neg = neg;
W.field = field;
W.index = index;
W.call = call;
W.mcall = mcall;
W.tryv = tryv;
W.iter = iter;
W.match = match;
W.arity = arity;
W.missing = missing;
W.argty = argty;
W.retty = retty;
W.meta = meta;
W.h = h;
W.hs = hs;
W.comp = comp;
W.display = display;
W.repr = repr;
W.short = short;
W.cls = (xs) => xs.filter((x) => !isNil(x) && x !== false).map(display).join(" ");
W.component = (name, params, render) => {
  render.$name = name;
  render.$p = params;
  render.$comp = true;
  C[name] = render;
  return render;
};
// `~name = live(fname, ...args)`: state that follows the server.
W.live = (inst, name, fname, args) => {
  if (inst.subs[name]) return;
  const info = inst.liveInfo && inst.liveInfo[name];
  const sub = { inst, name, fname, args, topics: info ? info.t : [] };
  inst.subs[name] = sub;
  Live.subs.add(sub);
  if (!inst.ready.has(name)) {
    inst.ready.add(name);
    inst.s[name] = null;
    liveRefresh(sub);
  } else {
    liveRestart();
  }
};
W.server = (name) => meta((...args) => rpc(name, args), name, null);
