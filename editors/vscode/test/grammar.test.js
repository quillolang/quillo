// Tokenizes Lite with VS Code's TextMate engine and checks key scopes.
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
    fs.readFileSync(path.join(__dirname, "../syntaxes/lite.tmLanguage.json"), "utf8"), "lite.tmLanguage.json"),
});

// [line, token text, scope that token must have]
const cases = [
  ['route GET /users/:id(int) = find_user(id)', "route", "keyword.other.route.lite"],
  ['route GET /users/:id(int) = find_user(id)', "GET", "constant.language.http-method.lite"],
  ['route GET /users/:id(int) = find_user(id)', "id", "variable.parameter.path.lite"],
  ['route GET /users/:id(int) = find_user(id)', "int", "support.type.lite"],
  ['route GET /users/:id(int) = find_user(id)', "find_user", "entity.name.function.call.lite"],
  ['fn add(a: int, b: int = 10) -> int = a + b', "add", "entity.name.function.lite"],
  ['fn add(a: int, b: int = 10) -> int = a + b', "10", "constant.numeric.integer.lite"],
  ['fn parse(s: str) -> int ! str with db, http = 1', "with", "keyword.other.with.lite"],
  ['fn parse(s: str) -> int ! str with db, http = 1', "db, http", "support.constant.effect.lite"],
  ['server fn save(text: str) = text', "server", "storage.modifier.server.lite"],
  ['server fn save(text: str) = text', "save", "entity.name.function.lite"],
  ['component Counter(start: int = 0) {', "Counter", "entity.name.type.component.lite"],
  ['    ~count = start', "count", "variable.other.state.lite"],
  ['let greeting = "Hello, {name}!"', "greeting", "variable.other.declaration.lite"],
  ['let greeting = "Hello, {name}!"', "Hello, ", "string.quoted.double.lite"],
  ['let greeting = "Hello, {name}!"', "name", "meta.interpolation.lite"],
  ['let e = "a \\{ b"', "\\{", "constant.character.escape.lite"],
  ['<li.todo #todo-{t.id} class={x}>"text"</li>', "li", "entity.name.tag.lite"],
  ['<li.todo #todo-{t.id} class={x}>"text"</li>', ".todo", "entity.other.attribute-name.class.lite"],
  ['<li.todo #todo-{t.id} class={x}>"text"</li>', "class", "entity.other.attribute-name.lite"],
  ['<li.todo #todo-{t.id} class={x}>"text"</li>', "text", "string.quoted.double.lite"],
  ['<button @click={fn() { count += 1 }}>"+"</button>', "click", "entity.other.attribute-name.event.lite"],
  ['<button @click={fn() { count += 1 }}>"+"</button>', "+=", "keyword.operator.assignment.lite"],
  ['<input bind:value={draft}/>', "bind", "keyword.other.bind.lite"],
  ['<Card title="x"/>', "Card", "entity.name.type.component.lite"],
  ['orders |> filter(fn(o) = o.total > 100) ?? []', "|>", "keyword.operator.pipe.lite"],
  ['orders |> filter(fn(o) = o.total > 100) ?? []', "??", "keyword.operator.coalesce.lite"],
  ['orders |> filter(fn(o) = o.total > 100) ?? []', "filter", "support.function.builtin.lite"],
  ['let rows = db.query("select 1")', "db", "support.module.lite"],
  ['let rows = db.query("select 1")', "query", "support.function.lite"],
  ['if x > 0 then "pos" else nil // done', "if", "keyword.control.lite"],
  ['if x > 0 then "pos" else nil // done', "nil", "constant.language.lite"],
  ['if x > 0 then "pos" else nil // done', "// done", "comment.line.double-slash.lite"],
  ['match e { { type: "click", x: int } => x }', "int", "support.type.pattern.lite"],
  ['let v = request.json', "request", "variable.language.request.lite"],
];

(async () => {
  const grammar = await registry.loadGrammar("source.lite");
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
  for (const f of fs.readdirSync(examples).filter((f) => f.endsWith(".lt"))) {
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
