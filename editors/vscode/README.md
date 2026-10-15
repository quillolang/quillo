# Lite for VS Code

Language support for [Lite](https://github.com/wisp-one/lite), the small web language.

- **Highlighting** for routes and paths, HTML literals (tags, `.class#id`, `@event`, `bind:value`),
  string interpolation, `~state`, types, effects and pipelines.
- **Diagnostics as you type**: unknown names, wrong arguments, type mismatches, effects,
  component props, code that cannot run in the browser, unused variables.
- **Hover** shows types and signatures; **Go to Definition** works across `use`d files.
- **Completion** for your functions and variables, builtins (with signatures), `db.`, `http.`
  and `json.` members, keywords, types, HTTP methods and effects.
- **Outline** of functions, views, components, variables and routes.
- **Snippets**: `route`, `routejson`, `fn`, `fnb`, `server fn`, `view`, `component`, `match`, `for`, `page`.
- **Commands**: *Lite: Run this file (watch mode)* (also the ▶ button), *Lite: Check this file*,
  *Lite: Restart the language server*.

The editor features come from `lite lsp`, the language server built into the `lite` binary.
The extension looks for the binary in the `lite.path` setting, then in `target/release` or
`target/debug` of the open workspace, then on your `PATH`.

## Install

```sh
cargo install --path .            # from the Lite repository: puts `lite` on your PATH
cd editors/vscode
npm install
npm run package                   # builds lite-lang-<version>.vsix
code --install-extension lite-lang-*.vsix
```

## Other editors

Any editor with LSP support can run `lite lsp` over stdio for `*.lt` files. For example, Neovim:

```lua
vim.filetype.add({ extension = { lite = "lite" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "lite",
  callback = function() vim.lsp.start({ name = "lite", cmd = { "lite", "lsp" } }) end,
})
```

Helix (`languages.toml`):

```toml
[language-server.lt]
command = "lite"
args = ["lsp"]

[[language]]
name = "lite"
scope = "source.lt"
file-types = ["lite"]
comment-token = "//"
language-servers = ["lite"]
```

## Development

`npm test` checks the grammar against sample lines and the example programs.
