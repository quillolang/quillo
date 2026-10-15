// Wisp for VS Code: starts `wisp lsp` and adds Run/Check commands.
const vscode = require("vscode");
const fs = require("fs");
const path = require("path");
const { LanguageClient } = require("vscode-languageclient/node");

let client = null;

/** The wisp binary: the setting, a build in the workspace, or `wisp` on the PATH. */
function wispPath() {
  const configured = vscode.workspace.getConfiguration("wisp").get("path");
  if (configured) return configured;
  for (const folder of vscode.workspace.workspaceFolders || []) {
    for (const p of ["target/release/wisp", "target/debug/wisp"]) {
      const full = path.join(folder.uri.fsPath, p);
      if (fs.existsSync(full)) return full;
    }
  }
  return "wisp";
}

async function startClient() {
  const command = wispPath();
  client = new LanguageClient(
    "wisp",
    "Wisp",
    { run: { command, args: ["lsp"] }, debug: { command, args: ["lsp"] } },
    {
      documentSelector: [{ scheme: "file", language: "wisp" }],
      synchronize: { fileEvents: vscode.workspace.createFileSystemWatcher("**/*.wisp") },
    }
  );
  try {
    await client.start();
  } catch (err) {
    client = null;
    const pick = await vscode.window.showWarningMessage(
      `Wisp: could not start "${command} lsp". Install wisp or set "wisp.path".`,
      "Open settings"
    );
    if (pick) vscode.commands.executeCommand("workbench.action.openSettings", "wisp.path");
  }
}

function terminal() {
  const existing = vscode.window.terminals.find((t) => t.name === "Wisp");
  return existing || vscode.window.createTerminal("Wisp");
}

function currentFile() {
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.document.languageId !== "wisp") {
    vscode.window.showInformationMessage("Open a .wisp file first.");
    return null;
  }
  return ed.document;
}

const quote = (s) => `"${s.replace(/(["\\$`])/g, "\\$1")}"`;

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("wisp.run", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const port = vscode.workspace.getConfiguration("wisp").get("port") || 8080;
      const t = terminal();
      t.show();
      t.sendText(`${quote(wispPath())} run ${quote(doc.fileName)} --watch --port ${port}`);
    }),
    vscode.commands.registerCommand("wisp.check", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const t = terminal();
      t.show();
      t.sendText(`${quote(wispPath())} check ${quote(doc.fileName)}`);
    }),
    vscode.commands.registerCommand("wisp.restartServer", async () => {
      if (client) await client.stop();
      await startClient();
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("wisp.path")) {
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
