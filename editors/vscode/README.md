# Wisp for VS Code

Language support for [Wisp](https://github.com/wisp-one/wisp), the small web language.

- **Highlighting** for routes and paths, HTML literals (tags, `.class#id`, `@event`, `bind:value`),
  string interpolation, `~state`, types, effects and pipelines.
- **Diagnostics as you type**: unknown names, wrong arguments, type mismatches, effects,
  component props, code that cannot run in the browser, unused variables.
- **Hover** shows types and signatures; **Go to Definition** works across `use`d files.
- **Completion** for your functions and variables, builtins (with signatures), `db.`, `http.`
  and `json.` members, keywords, types, HTTP methods and effects.
- **Outline** of functions, views, components, variables and routes.
- **Snippets**: `route`, `routejson`, `fn`, `fnb`, `server fn`, `view`, `component`, `match`, `for`, `page`.
- **Commands**: *Wisp: Run this file (watch mode)* (also the ▶ button), *Wisp: Check this file*,
  *Wisp: Restart the language server*.

The editor features come from `wisp lsp`, the language server built into the `wisp` binary.
The extension looks for the binary in the `wisp.path` setting, then in `target/release` or
`target/debug` of the open workspace, then on your `PATH`.

## Install

```sh
cargo install --path .            # from the Wisp repository: puts `wisp` on your PATH
cd editors/vscode
npm install
npm run package                   # builds wisp-lang-<version>.vsix
code --install-extension wisp-lang-*.vsix
```

## Other editors

Any editor with LSP support can run `wisp lsp` over stdio for `*.wisp` files. For example, Neovim:

```lua
vim.filetype.add({ extension = { wisp = "wisp" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "wisp",
  callback = function() vim.lsp.start({ name = "wisp", cmd = { "wisp", "lsp" } }) end,
})
```

Helix (`languages.toml`):

```toml
[language-server.wisp]
command = "wisp"
args = ["lsp"]

[[language]]
name = "wisp"
scope = "source.wisp"
file-types = ["wisp"]
comment-token = "//"
language-servers = ["wisp"]
```

## Development

`npm test` checks the grammar against sample lines and the example programs.
