import * as vscode from "vscode";
import { resolveBinary, validateMcpBinary } from "./binary";
import { Log } from "./settings";

export function registerMcp(context: vscode.ExtensionContext, log: Log): vscode.Disposable {
  const changed = new vscode.EventEmitter<void>();
  const provider = vscode.lm.registerMcpServerDefinitionProvider("dygnosis", {
    onDidChangeMcpServerDefinitions: changed.event,
    provideMcpServerDefinitions: async () => {
      try {
        const binary = await resolveBinary(context, log);
        return [new vscode.McpStdioServerDefinition("Dygnosis", binary.path, ["mcp"], {}, binary.version)];
      } catch (error) { log(`MCP discovery: ${String(error)}. Reinstall Dygnosis or configure dynare.serverPath.`); return []; }
    },
    resolveMcpServerDefinition: async () => {
      try {
        const binary = await resolveBinary(context, log);
        await validateMcpBinary(binary, log);
        return new vscode.McpStdioServerDefinition("Dygnosis", binary.path, ["mcp"], {}, binary.version);
      } catch (error) {
        throw new Error(`Dygnosis MCP could not start: ${String(error)}. Open dynare.serverPath, use the bundle, or show Dygnosis Output.`, { cause: error });
      }
    },
  });
  const listener = vscode.workspace.onDidChangeConfiguration(event => {
    if (event.affectsConfiguration("dynare.serverPath")) changed.fire();
  });
  return vscode.Disposable.from(provider, listener, changed);
}
