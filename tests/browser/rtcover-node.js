// Preloaded into every Node the tests start while runtime.js's coverage is
// measured (NODE_OPTIONS=--require, scripts/runtime-coverage.sh): records
// what of each script ran and, as it exits, writes what of runtime.js that
// was (rtcover.js).

"use strict";
const dir = process.env.QUILLO_RUNTIME_COVERAGE;
if (dir) {
  const inspector = require("inspector");
  const session = new inspector.Session();
  session.connect();
  // A session in the same thread answers before `post` returns.
  const post = (method, params) => {
    let out;
    let err;
    session.post(method, params, (e, r) => {
      err = e;
      out = r;
    });
    if (err) throw err;
    return out;
  };
  post("Profiler.enable");
  post("Profiler.startPreciseCoverage", { callCount: true, detailed: true });
  process.on("exit", () => {
    const rt = require("./rtcover");
    const { result } = post("Profiler.takePreciseCoverage");
    post("Debugger.enable");
    const lines = new Set();
    for (const s of result) {
      if (s.url.startsWith("node:") || s.url === __filename || s.url.endsWith("rtcover.js")) continue;
      let source;
      try {
        source = post("Debugger.getScriptSource", { scriptId: s.scriptId }).scriptSource;
      } catch (_) {
        continue;
      }
      const map = rt.mapScript(source);
      if (map) rt.hits(source, s.functions, map, lines);
    }
    rt.record(dir, "node", lines);
  });
}
