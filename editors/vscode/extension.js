// Quillo for VS Code: starts `quillo lsp` and adds Run/Check commands.
const vscode = require("vscode");
const fs = require("fs");
const path = require("path");
const { LanguageClient } = require("vscode-languageclient/node");

let client = null;

/** The quillo binary: the setting, a build in the workspace, or `quillo` on the PATH. */
function quilloPath() {
  const configured = vscode.workspace.getConfiguration("quillo").get("path");
  if (configured) return configured;
  for (const folder of vscode.workspace.workspaceFolders || []) {
    for (const p of ["target/release/quillo", "target/debug/quillo"]) {
      const full = path.join(folder.uri.fsPath, p);
      if (fs.existsSync(full)) return full;
    }
  }
  return "quillo";
}

async function startClient() {
  const command = quilloPath();
  client = new LanguageClient(
    "quillo",
    "Quillo",
    { run: { command, args: ["lsp"] }, debug: { command, args: ["lsp"] } },
    {
      // .qwer files are Rust (rust-analyzer's too, where it is installed);
      // the server adds what Quillo makes of them, and the compiler's errors.
      documentSelector: [{ scheme: "file", language: "quillo" }, { scheme: "file", pattern: "**/*.qwer" }],
      synchronize: { fileEvents: vscode.workspace.createFileSystemWatcher("**/*.{qwe,qwer}") },
    }
  );
  try {
    await client.start();
  } catch (err) {
    client = null;
    const pick = await vscode.window.showWarningMessage(
      `Quillo: could not start "${command} lsp". Install quillo or set "quillo.path".`,
      "Open settings"
    );
    if (pick) vscode.commands.executeCommand("workbench.action.openSettings", "quillo.path");
  }
}

function terminal() {
  const existing = vscode.window.terminals.find((t) => t.name === "Quillo");
  return existing || vscode.window.createTerminal("Quillo");
}

function currentFile() {
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.document.languageId !== "quillo") {
    vscode.window.showInformationMessage("Open a .quillo file first.");
    return null;
  }
  return ed.document;
}

const quote = (s) => `"${s.replace(/(["\\$`])/g, "\\$1")}"`;

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("quillo.run", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const port = vscode.workspace.getConfiguration("quillo").get("port") || 8080;
      const t = terminal();
      t.show();
      t.sendText(`${quote(quilloPath())} run ${quote(doc.fileName)} --watch --port ${port}`);
    }),
    vscode.commands.registerCommand("quillo.check", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const t = terminal();
      t.show();
      t.sendText(`${quote(quilloPath())} check ${quote(doc.fileName)}`);
    }),
    vscode.commands.registerCommand("quillo.restartServer", async () => {
      if (client) await client.stop();
      await startClient();
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("quillo.path")) {
        if (client) await client.stop();
        await startClient();
      }
    })
  );
  await startClient();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
