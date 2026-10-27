# Quillo for VS Code

Language support for [Quillo](https://github.com/wisp-one/quillo), the small web language.

- **Highlighting** for routes and paths, HTML literals (tags, `.class#id`, `@event`, `bind:value`),
  string interpolation, `state` declarations, `type` records, numbers such as `1e-9`, types,
  effects and pipelines.
- **Diagnostics as you type**: unknown names, wrong arguments, type mismatches, effects,
  component props, code that cannot run in the browser, unused variables.
- **Hover** shows types and signatures; **Go to Definition** works across `use`d files.
- **Completion** for your functions and variables, builtins (with signatures), the members of
  `db.`, `http.`, `json.`, `udp.`, `channel.`, `media.`, `crypto.`, `image.`, `math.`, `stats.`
  and `vec.`, keywords, types, HTTP methods and effects (`net` among them).
- **Outline** of functions, views, components, variables and routes.
- **Snippets**: `route`, `routejson`, `fn`, `fnb`, `server fn`, `view`, `component`, `match`, `for`, `page`,
  and for real time: `udplisten`, `channelon`, `channelopen`, `mediartp`, `quillo-media`.
- **File icons**: a quill for `.qwe` files, and a quill with a gear for `.qwer` files, in the
  default (Seti) icon theme and any other that has none of its own for them.
- **Rust in `.qwer` files**: highlighted as Rust, with the `//! [dependencies]` header as TOML;
  the Rust compiler's errors and warnings as you open and save (through `cargo check`);
  a warning on each `pub fn` Quillo cannot export, saying why; hover shows how Quillo sees an
  export, and the outline lists them. Their functions are checked, completed and hovered in
  the `.qwe` files that `use` them.
- **Debugging** (F5 in a `.qwe` file): breakpoints (with conditions, hit counts and logpoints),
  stepping, the stack, variables (locals, globals and the request), the debug console, and
  stopping where a runtime error happens. Through `quillo dap`; see Debugging in the user guide.
- **Commands**: *Quillo: Run this file (watch mode)* (also the ▶ button), *Quillo: Debug this file*,
  *Quillo: Check this file*, *Quillo: Restart the language server*.

The editor features come from `quillo lsp`, the language server built into the `quillo` binary,
and the debugger from `quillo dap`.
The extension looks for the binary in the `quillo.path` setting, then in `target/release` or
`target/debug` of the open workspace, then on your `PATH`.

## Install

```sh
cargo install --path .            # from the Quillo repository: puts `quillo` on your PATH
cd editors/vscode
npm install
npm run package                   # builds quillo-lang-<version>.vsix
code --install-extension quillo-lang-*.vsix
```

## Other editors

Any editor with LSP support can run `quillo lsp` over stdio for `*.qwe` files. For example, Neovim:

```lua
vim.filetype.add({ extension = { qwe = "quillo" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "quillo",
  callback = function() vim.lsp.start({ name = "quillo", cmd = { "quillo", "lsp" } }) end,
})
```

Helix (`languages.toml`):

```toml
[language-server.quillo]
command = "quillo"
args = ["lsp"]

[[language]]
name = "quillo"
scope = "source.quillo"
file-types = ["qwe"]
comment-token = "//"
language-servers = ["quillo"]
```

For debugging, any editor with a Debug Adapter Protocol client can run `quillo dap` (a launch
takes `program`, `args`, `cwd` and `stopOnEntry`). Neovim, with nvim-dap:

```lua
local dap = require("dap")
dap.adapters.quillo = { type = "executable", command = "quillo", args = { "dap" } }
dap.configurations.quillo = {
  { type = "quillo", request = "launch", name = "Debug this file", program = "${file}", args = { "--watch" } },
}
```

## Development

`npm test` checks the grammar against sample lines and the example programs.
