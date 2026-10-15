// Lite for VS Code: starts `lite lsp` and adds Run/Check commands.
const vscode = require("vscode");
const fs = require("fs");
const path = require("path");
const { LanguageClient } = require("vscode-languageclient/node");

let client = null;

/** The lite binary: the setting, a build in the workspace, or `lite` on the PATH. */
function litePath() {
  const configured = vscode.workspace.getConfiguration("lite").get("path");
  if (configured) return configured;
  for (const folder of vscode.workspace.workspaceFolders || []) {
    for (const p of ["target/release/lite", "target/debug/lite"]) {
      const full = path.join(folder.uri.fsPath, p);
      if (fs.existsSync(full)) return full;
    }
  }
  return "lite";
}

async function startClient() {
  const command = litePath();
  client = new LanguageClient(
    "lite",
    "Lite",
    { run: { command, args: ["lsp"] }, debug: { command, args: ["lsp"] } },
    {
      documentSelector: [{ scheme: "file", language: "lite" }],
      synchronize: { fileEvents: vscode.workspace.createFileSystemWatcher("**/*.lt") },
    }
  );
  try {
    await client.start();
  } catch (err) {
    client = null;
    const pick = await vscode.window.showWarningMessage(
      `Lite: could not start "${command} lsp". Install lite or set "lite.path".`,
      "Open settings"
    );
    if (pick) vscode.commands.executeCommand("workbench.action.openSettings", "lite.path");
  }
}

function terminal() {
  const existing = vscode.window.terminals.find((t) => t.name === "Lite");
  return existing || vscode.window.createTerminal("Lite");
}

function currentFile() {
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.document.languageId !== "lite") {
    vscode.window.showInformationMessage("Open a .lite file first.");
    return null;
  }
  return ed.document;
}

const quote = (s) => `"${s.replace(/(["\\$`])/g, "\\$1")}"`;

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("lite.run", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const port = vscode.workspace.getConfiguration("lite").get("port") || 8080;
      const t = terminal();
      t.show();
      t.sendText(`${quote(litePath())} run ${quote(doc.fileName)} --watch --port ${port}`);
    }),
    vscode.commands.registerCommand("lite.check", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const t = terminal();
      t.show();
      t.sendText(`${quote(litePath())} check ${quote(doc.fileName)}`);
    }),
    vscode.commands.registerCommand("lite.restartServer", async () => {
      if (client) await client.stop();
      await startClient();
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("lite.path")) {
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
