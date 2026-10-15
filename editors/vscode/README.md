# Quillo for VS Code

Language support for [Quillo](https://github.com/wisp-one/quillo), the small web language.

- **Highlighting** for routes and paths, HTML literals (tags, `.class#id`, `@event`, `bind:value`),
  string interpolation, `~state`, types, effects and pipelines.
- **Diagnostics as you type**: unknown names, wrong arguments, type mismatches, effects,
  component props, code that cannot run in the browser, unused variables.
- **Hover** shows types and signatures; **Go to Definition** works across `use`d files.
- **Completion** for your functions and variables, builtins (with signatures), `db.`, `http.`
  and `json.` members, keywords, types, HTTP methods and effects.
- **Outline** of functions, views, components, variables and routes.
- **Snippets**: `route`, `routejson`, `fn`, `fnb`, `server fn`, `view`, `component`, `match`, `for`, `page`.
- **Commands**: *Quillo: Run this file (watch mode)* (also the ▶ button), *Quillo: Check this file*,
  *Quillo: Restart the language server*.

The editor features come from `quillo lsp`, the language server built into the `quillo` binary.
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

## Development

`npm test` checks the grammar against sample lines and the example programs.
