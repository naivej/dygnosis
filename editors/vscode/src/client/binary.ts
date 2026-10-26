import { execFile, spawn } from "node:child_process";
import { stat } from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";
import { Log } from "./settings";
import { record } from "./protocol";
import { ActionFailure } from "./failure";

export interface Binary { path: string; override: boolean; version: string }
export interface McpBinary extends Binary { args: string[] }
export function executablePath(extensionPath: string, globalValue: unknown, platform = process.platform): { path: string; override: boolean } {
  if (globalValue !== undefined && typeof globalValue !== "string") throw new Error("dynare.serverPath must be an absolute path or empty.");
  const override = typeof globalValue === "string" && globalValue.trim() !== "";
  const selected = override ? globalValue.trim() : path.join(extensionPath, "bin", platform === "win32" ? "dygnosis.exe" : "dygnosis");
  if (!path.isAbsolute(selected)) throw new Error("dynare.serverPath must be an absolute path.");
  return { path: selected, override };
}
export function mcpExecutablePath(extensionPath: string, platform = process.platform): string {
  return path.join(extensionPath, "bin", platform === "win32" ? "dygnosis-mcp.exe" : "dygnosis-mcp");
}
function selectedPath(context: vscode.ExtensionContext, log: Log): { path: string; override: boolean } {
  const inspected = vscode.workspace.getConfiguration("dynare").inspect<unknown>("serverPath");
  if (inspected?.workspaceValue !== undefined || inspected?.workspaceFolderValue !== undefined) log("Ignored workspace dynare.serverPath; only a user/machine override can choose an executable.");
  return executablePath(context.extensionPath, inspected?.globalValue);
}
async function binaryVersion(context: vscode.ExtensionContext, selected: { path: string; override: boolean }, label: string): Promise<Binary> {
  const facts = await stat(selected.path).catch(error => { throw new ActionFailure(`Cannot read Dygnosis ${label} ${selected.path}: ${error instanceof Error ? error.message : String(error)}`, "ENGINE_PATH_UNAVAILABLE", error); });
  if (!facts.isFile()) throw new ActionFailure(`Dygnosis ${label} is not a file: ${selected.path}`, "ENGINE_PATH_UNAVAILABLE");
  const manifest: unknown = context.extension.packageJSON;
  const version = record(manifest) && typeof manifest.version === "string" ? manifest.version : "unknown";
  return { ...selected, version: `${version}:${selected.path}:${facts.size}:${facts.mtimeMs}` };
}
export async function resolveBinary(context: vscode.ExtensionContext, log: Log): Promise<Binary> {
  return binaryVersion(context, selectedPath(context, log), "executable");
}
export async function resolveMcpBinary(context: vscode.ExtensionContext, log: Log): Promise<McpBinary> {
  const selected = selectedPath(context, log);
  const executable = selected.override ? selected.path : mcpExecutablePath(context.extensionPath);
  const binary = await binaryVersion(context, { ...selected, path: executable }, "MCP executable");
  return { ...binary, args: ["mcp"] };
}
function stderrExcerpt(stderr: string): string {
  return stderr.trim().replace(/\s+/g, " ").slice(0, 640);
}
function runVersion(executable: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(executable, ["--version"], { timeout: 5000, windowsHide: true, encoding: "utf8" }, (error, stdout, stderr) => {
      if (error) {
        const excerpt = stderrExcerpt(stderr);
        reject(new ActionFailure(`Cannot query ${executable}: ${error.message}${excerpt ? ` stderr: ${excerpt}` : ""}`, "ENGINE_START_FAILED", error));
      } else resolve(stdout.trim());
    });
  });
}
export async function validateMcpBinary(binary: McpBinary, log: Log): Promise<void> {
  const version = await runVersion(binary.path);
  const validVersion = binary.override ? /^dygnosis(?:-mcp)?\s+\d+\./i : /^dygnosis-mcp\s+\d+\./i;
  if (!validVersion.test(version)) throw new ActionFailure(`MCP engine ${binary.path} reported ${version.slice(0, 100) || "no version"}; a Dygnosis MCP engine is required.`, "ENGINE_INCOMPATIBLE");
  log(`MCP executable: ${binary.path} (${version})`);
  await new Promise<void>((resolve, reject) => {
    const child = spawn(binary.path, binary.args, { windowsHide: true, shell: false, stdio: "pipe" });
    let buffer = "", stderr = "", stderrBytes = 0, settled = false;
    const finish = (error?: Error): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.kill();
      if (error) {
        if (stderr.trim()) log(`MCP stderr (${binary.path}): ${stderr.trim()}`);
        const excerpt = stderrExcerpt(stderr);
        const detail = excerpt ? ` MCP stderr: ${excerpt}` : "";
        reject(new ActionFailure(`MCP initialization for ${binary.path}: ${error.message}${detail}`, error instanceof ActionFailure ? error.code : "ENGINE_START_FAILED", error));
      } else resolve();
    };
    const timer = setTimeout(() => finish(new ActionFailure("No initialization response within 5 seconds. Open Dygnosis Output for engine messages.", "ENGINE_TIMEOUT")), 5000);
    child.on("error", error => finish(error));
    child.on("exit", (code, signal) => finish(new Error(`The process exited ${code === null ? `with signal ${signal ?? "unknown"}` : `with code ${code}`} before initialization completed. Open Dygnosis Output for engine messages.`)));
    child.stderr.on("data", (data: Buffer) => {
      const chunk = data.subarray(0, 16 * 1024 - stderrBytes); stderrBytes += chunk.length;
      stderr += chunk.toString();
    });
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
            finish(new ActionFailure("The engine returned incompatible MCP capabilities.", "ENGINE_INCOMPATIBLE"));
          } else finish();
        } catch (error) { finish(new Error("The selected engine returned invalid MCP JSON.", { cause: error })); }
      }
    });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "dygnosis-vscode-check", version: "1" } } }) + "\n");
  });
}
