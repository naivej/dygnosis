import * as vscode from "vscode";
import { resolveMcpBinary, validateMcpBinary } from "../client/binary";
import { Log } from "../client/settings";
import { failureText } from "../client/failure";

export function registerMcp(context: vscode.ExtensionContext, log: Log): vscode.Disposable {
  const changed = new vscode.EventEmitter<void>();
  const provider = vscode.lm.registerMcpServerDefinitionProvider("dygnosis", {
    onDidChangeMcpServerDefinitions: changed.event,
    provideMcpServerDefinitions: async () => {
      try {
        const binary = await resolveMcpBinary(context, log);
        return [new vscode.McpStdioServerDefinition("Dygnosis", binary.path, [], {}, binary.version)];
      } catch (error) { log(`MCP discovery failed: ${failureText(error)}`); throw error; }
    },
    resolveMcpServerDefinition: async () => {
      try {
        const binary = await resolveMcpBinary(context, log);
        await validateMcpBinary(binary, log);
        return new vscode.McpStdioServerDefinition("Dygnosis", binary.path, [], {}, binary.version);
      } catch (error) {
        log(`MCP start failed: ${failureText(error)}`); throw error;
      }
    },
  });
  const listener = vscode.workspace.onDidChangeConfiguration(event => {
    if (event.affectsConfiguration("dynare.serverPath")) changed.fire();
  });
  return vscode.Disposable.from(provider, listener, changed);
}
