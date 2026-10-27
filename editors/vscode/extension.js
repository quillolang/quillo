// Quillo for VS Code: starts `quillo lsp`, debugs with `quillo dap`, and adds
// Run/Check/Debug commands.
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
      // .qwer files are Rust: the server says what Quillo makes of them, and
      // what the compiler says (through `cargo check`).
      documentSelector: [{ scheme: "file", language: "quillo" }, { scheme: "file", language: "qwer" }],
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
    vscode.window.showInformationMessage("Open a .qwe file first.");
    return null;
  }
  return ed.document;
}

/** The debugger: `quillo dap` runs the program and speaks the Debug Adapter Protocol. */
const debugAdapters = {
  createDebugAdapterDescriptor(session) {
    const cwd = session.configuration.cwd || (session.workspaceFolder && session.workspaceFolder.uri.fsPath);
    return new vscode.DebugAdapterExecutable(quilloPath(), ["dap"], cwd ? { cwd } : {});
  },
};

/**
 * `"browser": true`: once the server says where it listens, the page opens in
 * Chrome (or Edge) with VS Code's JavaScript debugger, as part of this session.
 * The components come as files of their own with source maps (the server is
 * in development mode), so breakpoints in .qwe files stop in the browser too.
 */
const browserTrackers = {
  createDebugAdapterTracker(session) {
    const want = session.configuration.browser;
    if (!want) return undefined;
    let opened = false;
    return {
      onDidSendMessage(m) {
        if (opened || m.type !== "event" || m.event !== "output") return;
        const found = /listening on (https?:\/\/\S+)/.exec(m.body.output || "");
        if (!found) return;
        opened = true;
        const at = found[1].replace("//0.0.0.0", "//localhost").replace("//[::]", "//localhost");
        const url = session.configuration.url || at;
        const cwd = session.configuration.cwd || (session.workspaceFolder && session.workspaceFolder.uri.fsPath);
        vscode.debug.startDebugging(
          session.workspaceFolder,
          {
            type: want === "edge" ? "msedge" : "chrome",
            request: "launch",
            name: "Pages",
            url,
            webRoot: cwd,
            sourceMaps: true,
            pauseForSourceMap: true,
            // The runtime: stepping goes through it to the program's code.
            skipFiles: ["**/_quillo/src/*/quillo.js", "**/_quillo/src/*/program*.js", "**/_quillo/src/*/boot.js"],
          },
          { parentSession: session, lifecycleManagedByParent: true, compact: true }
        );
      },
    };
  },
};

/** F5 with no launch.json: debug the open .qwe file. */
const debugConfigurations = {
  resolveDebugConfiguration(folder, config) {
    if (!config.type && !config.request && !config.name) {
      const ed = vscode.window.activeTextEditor;
      if (!ed || ed.document.languageId !== "quillo") return config;
      return { type: "quillo", request: "launch", name: "Debug this file", program: ed.document.fileName, args: ["--watch"] };
    }
    if (!config.program) {
      return vscode.window.showInformationMessage("Set \"program\" to the .qwe file to debug.").then(() => undefined);
    }
    return config;
  },
};

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
    vscode.commands.registerCommand("quillo.debug", async () => {
      const doc = currentFile();
      if (!doc) return;
      await doc.save();
      const port = vscode.workspace.getConfiguration("quillo").get("port") || 8080;
      await vscode.debug.startDebugging(vscode.workspace.getWorkspaceFolder(doc.uri), {
        type: "quillo",
        request: "launch",
        name: "Debug this file",
        program: doc.fileName,
        args: ["--watch", "--port", String(port)],
      });
    }),
    vscode.debug.registerDebugAdapterDescriptorFactory("quillo", debugAdapters),
    vscode.debug.registerDebugConfigurationProvider("quillo", debugConfigurations),
    vscode.debug.registerDebugAdapterTrackerFactory("quillo", browserTrackers),
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
