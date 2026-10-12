import { execFile, spawn } from "node:child_process";
import { stat } from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";
import { Log } from "./settings";
import { record } from "./protocol";

export interface Binary { path: string; override: boolean; version: string }
export function executablePath(extensionPath: string, globalValue: unknown, platform = process.platform): { path: string; override: boolean } {
  if (globalValue !== undefined && typeof globalValue !== "string") throw new Error("dynare.serverPath must be an absolute path or empty.");
  const override = typeof globalValue === "string" && globalValue.trim() !== "";
  const selected = override ? globalValue.trim() : path.join(extensionPath, "bin", platform === "win32" ? "dygnosis.exe" : "dygnosis");
  if (!path.isAbsolute(selected)) throw new Error("dynare.serverPath must be an absolute path.");
  return { path: selected, override };
}
export async function resolveBinary(context: vscode.ExtensionContext, log: Log): Promise<Binary> {
  const inspected = vscode.workspace.getConfiguration("dynare").inspect<unknown>("serverPath");
  if (inspected?.workspaceValue !== undefined || inspected?.workspaceFolderValue !== undefined) log("Ignored workspace dynare.serverPath; only a user/machine override can choose an executable.");
  const selected = executablePath(context.extensionPath, inspected?.globalValue);
  const facts = await stat(selected.path);
  if (!facts.isFile()) throw new Error(`Dygnosis executable is not a file: ${selected.path}`);
  const manifest: unknown = context.extension.packageJSON;
  const version = record(manifest) && typeof manifest.version === "string" ? manifest.version : "unknown";
  return { ...selected, version: `${version}:${selected.path}:${facts.size}:${facts.mtimeMs}` };
}
function runVersion(executable: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(executable, ["--version"], { timeout: 5000, windowsHide: true }, (error, stdout) => {
      if (error) reject(new Error(error.message, { cause: error })); else resolve(stdout.trim());
    });
  });
}
export async function validateMcpBinary(binary: Binary, log: Log): Promise<void> {
  const version = await runVersion(binary.path);
  if (!/^dygnosis\s+\d+\./i.test(version)) throw new Error(`The selected executable is not a compatible Dygnosis MCP engine: ${binary.path}. Open dynare.serverPath or use the bundled binary.`);
  log(`MCP executable: ${binary.path} (${version})`);
  await new Promise<void>((resolve, reject) => {
    const child = spawn(binary.path, ["mcp"], { windowsHide: true, shell: false, stdio: "pipe" });
    let buffer = "", settled = false;
    const finish = (error?: Error): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.kill();
      if (error) reject(error); else resolve();
    };
    const timer = setTimeout(() => finish(new Error("The selected Dygnosis engine did not answer MCP initialization within 5 seconds. Update dynare.serverPath or use the bundled binary.")), 5000);
    child.on("error", error => finish(error));
    child.on("exit", () => finish(new Error("The selected Dygnosis engine exited before completing MCP initialization.")));
    child.stdout.on("data", (data: Buffer) => {
      buffer += data.toString();
      if (buffer.length > 1024 * 1024) { finish(new Error("The selected engine returned an invalid MCP response.")); return; }
      let newline: number;
      while ((newline = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, newline); buffer = buffer.slice(newline + 1);
        try {
          const response: unknown = JSON.parse(line);
          if (!record(response) || response.id !== 1) continue;
          if (!record(response.result) || !record(response.result.capabilities) || !record(response.result.capabilities.tools) || !record(response.result.serverInfo) || response.result.serverInfo.name !== "dygnosis") {
            finish(new Error("The selected engine is incompatible with the Dygnosis MCP provider. Update dynare.serverPath or use the bundled binary."));
          } else finish();
        } catch (error) { finish(new Error("The selected engine returned invalid MCP JSON.", { cause: error })); }
      }
    });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "dygnosis-vscode-check", version: "1" } } }) + "\n");
  });
}
