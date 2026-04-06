import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import {
  ExtensionContext,
  workspace,
  window,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

function findPact(): string | undefined {
  // Check common locations
  const candidates = [
    path.join(os.homedir(), "bin", "pact"),
    path.join(os.homedir(), ".local", "bin", "pact"),
    "/usr/local/bin/pact",
    "/usr/bin/pact",
  ];

  // Also check workspace target/release and target/debug
  const workspaceFolders = workspace.workspaceFolders;
  if (workspaceFolders) {
    for (const folder of workspaceFolders) {
      candidates.unshift(
        path.join(folder.uri.fsPath, "target", "release", "pact"),
        path.join(folder.uri.fsPath, "target", "debug", "pact"),
      );
    }
  }

  for (const p of candidates) {
    if (fs.existsSync(p)) {
      return p;
    }
  }
  return undefined;
}

const outputChannel = window.createOutputChannel("PACT Language Server");

export function activate(context: ExtensionContext) {
  outputChannel.appendLine("PACT extension activating...");

  const config = workspace.getConfiguration("pact");
  const pactPath = config.get<string>("path") || findPact() || "pact";

  outputChannel.appendLine(`Using pact binary: ${pactPath}`);
  outputChannel.appendLine(`Exists: ${fs.existsSync(pactPath)}`);

  if (!fs.existsSync(pactPath) && pactPath === "pact") {
    window.showWarningMessage(
      "PACT binary not found. Install pact or set 'pact.path' in settings. LSP features disabled."
    );
    return;
  }

  const serverOptions: ServerOptions = {
    command: pactPath,
    args: ["lsp"],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "pact" }],
    outputChannel,
  };

  outputChannel.appendLine(`Starting LSP: ${pactPath} lsp`);

  client = new LanguageClient(
    "pact-lsp",
    "PACT Language Server",
    serverOptions,
    clientOptions
  );

  client.start().then(
    () => outputChannel.appendLine("LSP client started successfully"),
    (err) => outputChannel.appendLine(`LSP client failed to start: ${err}`)
  );
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
