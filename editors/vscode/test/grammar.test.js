// Tokenizes Quillo with VS Code's TextMate engine and checks key scopes.
// Run: npm test
const fs = require("fs");
const path = require("path");
const vsctm = require("vscode-textmate");
const oniguruma = require("vscode-oniguruma");

const wasm = fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer;
const onigLib = oniguruma.loadWASM(wasm).then(() => ({
  createOnigScanner: (s) => new oniguruma.OnigScanner(s),
  createOnigString: (s) => new oniguruma.OnigString(s),
}));
const registry = new vsctm.Registry({
  onigLib,
  loadGrammar: async () => vsctm.parseRawGrammar(
    fs.readFileSync(path.join(__dirname, "../syntaxes/quillo.tmLanguage.json"), "utf8"), "quillo.tmLanguage.json"),
});

// [line, token text, scope that token must have]
const cases = [
  ['route GET /users/:id(int) = find_user(id)', "route", "keyword.other.route.quillo"],
  ['route GET /users/:id(int) = find_user(id)', "GET", "constant.language.http-method.quillo"],
  ['route GET /users/:id(int) = find_user(id)', "id", "variable.parameter.path.quillo"],
  ['route GET /users/:id(int) = find_user(id)', "int", "support.type.quillo"],
  ['route GET /users/:id(int) = find_user(id)', "find_user", "entity.name.function.call.quillo"],
  ['fn add(a: int, b: int = 10) -> int = a + b', "add", "entity.name.function.quillo"],
  ['fn add(a: int, b: int = 10) -> int = a + b', "10", "constant.numeric.integer.quillo"],
  ['fn parse(s: str) -> int ! str with db, http = 1', "with", "keyword.other.with.quillo"],
  ['fn parse(s: str) -> int ! str with db, http = 1', "db, http", "support.constant.effect.quillo"],
  ['server fn save(text: str) = text', "server", "storage.modifier.server.quillo"],
  ['server fn save(text: str) = text', "save", "entity.name.function.quillo"],
  ['component Counter(start: int = 0) {', "Counter", "entity.name.type.component.quillo"],
  ['    ~count = start', "count", "variable.other.state.quillo"],
  ['let greeting = `Hello, ${name}!`', "greeting", "variable.other.declaration.quillo"],
  ['let greeting = `Hello, ${name}!`', "Hello, ", "string.template.quillo"],
  ['let greeting = `Hello, ${name}!`', "name", "meta.template.expression.quillo"],
  ['let plain = "no {interp} here"', "no {interp} here", "string.quoted.double.quillo"],
  ["let s = 'single'", "single", "string.quoted.single.quillo"],
  ['let e = "a \\" b"', '\\"', "constant.character.escape.quillo"],
  ['<li.todo #todo-{t.id} class={x}>text</li>', "li", "entity.name.tag.quillo"],
  ['<li.todo #todo-{t.id} class={x}>text</li>', ".todo", "entity.other.attribute-name.class.quillo"],
  ['<li.todo #todo-{t.id} class={x}>text</li>', "class", "entity.other.attribute-name.quillo"],
  ['<li.todo #todo-{t.id} class={x}>text</li>', "text", "meta.jsx.children.quillo"],
  ['<p>if you want, return {count} for null</p>', "if you want, return ", "meta.jsx.children.quillo"],
  ['<p>Fish &amp; chips</p>', "&amp;", "constant.character.entity.quillo"],
  ['<p>a <b>{n}</b> c</p>', "b", "entity.name.tag.quillo"],
  ['<button @click={() => { count += 1 }}>+</button>', "click", "entity.other.attribute-name.event.quillo"],
  ['<button @click={() => { count += 1 }}>+</button>', "+=", "keyword.operator.assignment.quillo"],
  ['<button @click={() => { count += 1 }}>+</button>', "=>", "storage.type.function.arrow.quillo"],
  ['<input bind:value={draft}/>', "bind", "keyword.other.bind.quillo"],
  ['<Card title="x"/>', "Card", "entity.name.type.component.quillo"],
  ['orders |> filter(o => o.total > 100 && o.ok) ?? []', "|>", "keyword.operator.pipe.quillo"],
  ['orders |> filter(o => o.total > 100 && o.ok) ?? []', "??", "keyword.operator.coalesce.quillo"],
  ['orders |> filter(o => o.total > 100 && o.ok) ?? []', "&&", "keyword.operator.logical.quillo"],
  ['orders |> filter(o => o.total > 100 && o.ok) ?? []', "o", "variable.parameter.quillo"],
  ['orders |> filter(o => o.total > 100 && o.ok) ?? []', "filter", "support.function.builtin.quillo"],
  ['let rows = db.query("select 1")', "db", "support.module.quillo"],
  ['let rows = db.query("select 1")', "query", "support.function.quillo"],
  ['let size = n > 0 ? "pos" : null // done', "?", "keyword.operator.ternary.quillo"],
  ['let size = n > 0 ? "pos" : null // done', "null", "constant.language.quillo"],
  ['let size = n > 0 ? "pos" : null // done', "// done", "comment.line.double-slash.quillo"],
  ['if !ok { return 1 }', "if", "keyword.control.quillo"],
  ['if !ok { return 1 }', "!", "keyword.operator.logical.quillo"],
  ['match e { { type: "click", x: int } => x }', "int", "support.type.pattern.quillo"],
  ['let v = request.json', "request", "variable.language.request.quillo"],
];

(async () => {
  const grammar = await registry.loadGrammar("source.quillo");
  let failed = 0;
  for (const [line, text, scope] of cases) {
    const { tokens } = grammar.tokenizeLine(line, vsctm.INITIAL);
    const tok = tokens.find((t) => line.slice(t.startIndex, t.endIndex) === text)
      || tokens.find((t) => line.slice(t.startIndex, t.endIndex).includes(text));
    const scopes = tok ? tok.scopes : [];
    if (!scopes.includes(scope)) {
      failed++;
      console.log(`FAIL ${JSON.stringify(text)} in ${JSON.stringify(line)}\n  want ${scope}\n  got  ${scopes.join(" ")}`);
    }
  }
  // Every example file tokenizes without leaving a tag or string open.
  const examples = path.join(__dirname, "../../../examples");
  for (const f of fs.readdirSync(examples).filter((f) => f.endsWith(".qwe"))) {
    let state = vsctm.INITIAL;
    for (const line of fs.readFileSync(path.join(examples, f), "utf8").split("\n")) {
      state = grammar.tokenizeLine(line, state).ruleStack;
    }
    if (state.depth > 1) {
      failed++;
      console.log(`FAIL ${f}: tokenizer state still nested at end of file (depth ${state.depth})`);
    }
  }
  console.log(failed ? `${failed} failed` : `all ${cases.length} grammar cases passed`);
  process.exit(failed ? 1 : 0);
})();
