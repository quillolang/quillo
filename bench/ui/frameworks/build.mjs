// Builds each framework's app into dist/<name>.js and dist/<name>.html,
// minified, for production, as js-framework-benchmark builds them.
//
//   npm install --ignore-scripts && node build.mjs
import * as esbuild from "esbuild";
import { transformAsync } from "@babel/core";
import { compile } from "svelte/compiler";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const here = path.dirname(new URL(import.meta.url).pathname);
const require = createRequire(import.meta.url);
const dist = path.join(here, "dist");
fs.mkdirSync(dist, { recursive: true });

const solid = {
  name: "solid",
  setup(b) {
    b.onLoad({ filter: /solid\.jsx$/ }, async (a) => {
      const src = fs.readFileSync(a.path, "utf8");
      const { code } = await transformAsync(src, { filename: a.path, presets: [[require.resolve("babel-preset-solid"), {}]] });
      return { contents: code, loader: "js" };
    });
  },
};
const svelte = {
  name: "svelte",
  setup(b) {
    b.onLoad({ filter: /\.svelte$/ }, (a) => {
      const { js } = compile(fs.readFileSync(a.path, "utf8"), { filename: a.path, generate: "client", css: "injected" });
      return { contents: js.code, loader: "js", resolveDir: path.dirname(a.path) };
    });
  },
};

const apps = {
  solid: { entry: "src/solid.jsx", plugins: [solid] },
  svelte: { entry: "src/svelte.js", plugins: [svelte], conditions: ["svelte", "browser", "production"] },
  react: { entry: "src/react.jsx", jsx: "automatic", jsxImportSource: "react" },
  preact: { entry: "src/preact.jsx", jsx: "automatic", jsxImportSource: "preact" },
  vue: { entry: "src/vue.js", alias: { vue: "vue/dist/vue.esm-bundler.js" } },
};

for (const [name, a] of Object.entries(apps)) {
  await esbuild.build({
    entryPoints: [path.join(here, a.entry)],
    bundle: true,
    minify: true,
    format: "iife",
    target: "es2022",
    outfile: path.join(dist, `${name}.js`),
    plugins: a.plugins || [],
    jsx: a.jsx,
    jsxImportSource: a.jsxImportSource,
    alias: a.alias,
    conditions: a.conditions,
    define: {
      "process.env.NODE_ENV": '"production"',
      __VUE_OPTIONS_API__: "true",
      __VUE_PROD_DEVTOOLS__: "false",
      __VUE_PROD_HYDRATION_MISMATCH_DETAILS__: "false",
    },
    logLevel: "warning",
  });
  fs.writeFileSync(path.join(dist, `${name}.html`), `<!doctype html>\n<html><head><meta charset="utf-8"><title>${name} keyed</title></head><body><div id="main"></div><script src="${name}.js"></script></body></html>\n`);
  console.log(name.padEnd(8), (fs.statSync(path.join(dist, `${name}.js`)).size / 1024).toFixed(1), "KB");
}
