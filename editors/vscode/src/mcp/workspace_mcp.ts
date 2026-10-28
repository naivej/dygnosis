import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";
import { resolveMcpBinary, validateMcpBinary } from "../client/binary";
import { record } from "../client/protocol";
import { Log } from "../client/settings";

const commandId = "dygnosis.setupWorkspaceMcp";
const reviewScheme = "dygnosis-mcp-review";
const targets: Record<string, string> = {
  "win32-x64": "x86_64-pc-windows-msvc", "win32-arm64": "aarch64-pc-windows-msvc",
  "darwin-x64": "x86_64-apple-darwin", "darwin-arm64": "aarch64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu",
};
function digest(bytes: Uint8Array): string { return createHash("sha256").update(bytes).digest("hex"); }
function errorCode(error: unknown): string | undefined { return record(error) && typeof error.code === "string" ? error.code : undefined; }
function delay(ms: number): Promise<void> { return new Promise(resolve => setTimeout(resolve, ms)); }
async function optionalBytes(file: string): Promise<Buffer | undefined> {
  try {
    const facts = await fs.lstat(file);
    if (!facts.isFile() || facts.isSymbolicLink()) throw new Error(`Expected a regular file: ${file}`);
    return await fs.readFile(file);
  } catch (error) { if (errorCode(error) === "ENOENT") return undefined; throw error; }
}
function sameBytes(a: Buffer | undefined, b: Buffer | undefined): boolean { return a === undefined ? b === undefined : b !== undefined && a.equals(b); }

/** Exclusive local-host lock. Stale removal is serialized to avoid deleting a new owner's lock. */
export async function withWorkspaceLock<T>(file: string, work: () => Promise<T>, attempts = 50, delayMs = 100): Promise<T> {
  const token = `${process.pid}:${randomUUID()}`;
  for (let attempt = 0; attempt < attempts; ++attempt) {
    let handle;
    try { handle = await fs.open(file, "wx", 0o600); }
    catch (error) {
      if (errorCode(error) !== "EEXIST") throw error;
      await recoverDeadLock(file);
      if (attempt + 1 < attempts) await delay(delayMs);
      continue;
    }
    try {
      await handle.writeFile(JSON.stringify({ pid: process.pid, token }));
      await handle.sync();
      return await work();
    } finally {
      await handle.close();
      const bytes = await optionalBytes(file);
      if (bytes && bytes.toString().includes(token)) await fs.unlink(file);
    }
  }
  throw new Error(`Another Dygnosis window is updating ${file}. Retry after it finishes.`);
}
async function recoverDeadLock(file: string): Promise<void> {
  const recovery = `${file}.recovery`;
  let handle;
  try { handle = await fs.open(recovery, "wx", 0o600); }
  catch (error) { if (errorCode(error) === "EEXIST") return; throw error; }
  try {
    const bytes = await optionalBytes(file);
    if (!bytes) return;
    let owner: unknown;
    try { owner = JSON.parse(bytes.toString()); } catch { return; }
    if (!record(owner) || typeof owner.pid !== "number" || !Number.isSafeInteger(owner.pid) || owner.pid <= 0) return;
    try { process.kill(owner.pid, 0); }
    catch (error) { if (errorCode(error) === "ESRCH") await fs.unlink(file); }
  } finally { await handle.close(); await fs.unlink(recovery); }
}

async function atomicBytes(file: string, bytes: Uint8Array, mode = 0o600, beforeCommit?: () => Promise<void>, commit: (staged: string, destination: string) => Promise<void> = fs.rename): Promise<void> {
  const temporary = `${file}.${randomUUID()}.tmp`;
  try {
    const handle = await fs.open(temporary, "wx", mode);
    try { await handle.writeFile(bytes); await handle.sync(); } finally { await handle.close(); }
    await beforeCommit?.();
    await commit(temporary, file);
  } finally { await fs.rm(temporary, { force: true }); }
}

type Version = { core: number[]; pre: string[] };
function versionParts(value: string): Version {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(value);
  if (!match) throw new Error(`Invalid engine version: ${value}`);
  const core = match.slice(1, 4).map(Number), pre = match[4]?.split(".") ?? [];
  if (core.some(part => !Number.isSafeInteger(part)) || pre.some(part => !part || /^\d+$/.test(part) && part.length > 1 && part.startsWith("0"))) throw new Error(`Invalid engine version: ${value}`);
  return { core, pre };
}
export function compareVersions(a: string, b: string): number {
  const left = versionParts(a), right = versionParts(b);
  for (let i = 0; i < 3; ++i) if (left.core[i] !== right.core[i]) return left.core[i] < right.core[i] ? -1 : 1;
  if (!left.pre.length || !right.pre.length) return left.pre.length === right.pre.length ? 0 : left.pre.length ? -1 : 1;
  for (let i = 0; i < Math.max(left.pre.length, right.pre.length); ++i) {
    const x = left.pre[i], y = right.pre[i];
    if (x === y) continue;
    if (x === undefined || y === undefined) return x === undefined ? -1 : 1;
    const nx = /^\d+$/.test(x), ny = /^\d+$/.test(y);
    if (nx && ny) return BigInt(x) < BigInt(y) ? -1 : 1;
    if (nx !== ny) return nx ? -1 : 1;
    return x < y ? -1 : 1;
  }
  return 0;
}
export interface PackageProvenance extends Record<string, unknown> {
  schema_version: 1; name: string; publisher: string; version: string; target: string;
  rust_target: string; commit: string; source: string; source_archive: string; binary_sha256: string;
  mcp_binary_name: string; mcp_binary_sha256: string; mcp_binary_size: number;
}
function provenance(value: unknown, manifest?: { name: string; publisher: string }): PackageProvenance {
  if (!record(value) || value.schema_version !== 1 || value.name !== "dygnosis" || typeof value.publisher !== "string" || !value.publisher
    || typeof value.version !== "string" || typeof value.target !== "string" || !targets[value.target]
    || value.rust_target !== targets[value.target] || typeof value.commit !== "string" || !/^[a-f0-9]{40}$/.test(value.commit)
    || typeof value.binary_sha256 !== "string" || !/^[a-f0-9]{64}$/.test(value.binary_sha256)
    || value.mcp_binary_name !== (String(value.target).startsWith("win32-") ? "dygnosis-mcp.exe" : "dygnosis-mcp")
    || typeof value.mcp_binary_sha256 !== "string" || !/^[a-f0-9]{64}$/.test(value.mcp_binary_sha256)
    || typeof value.mcp_binary_size !== "number" || !Number.isSafeInteger(value.mcp_binary_size) || value.mcp_binary_size <= 0
    || value.source !== `https://github.com/naivej/dygnosis/tree/${value.commit}`
    || value.source_archive !== `https://github.com/naivej/dygnosis/archive/${value.commit}.tar.gz`
    || typeof value.release !== "boolean" || typeof value.dirty !== "boolean"
    || value.release && (value.tag !== `v${value.version}` || value.dirty)
    || manifest && (value.name !== manifest.name || value.publisher !== manifest.publisher)) throw new Error("Dygnosis package provenance is missing or invalid. Reinstall the extension.");
  versionParts(value.version);
  return value as PackageProvenance;
}
function executableVersion(file: string): Promise<string> {
  return new Promise((resolve, reject) => execFile(file, ["--version"], { timeout: 5000, windowsHide: true }, (error, stdout) => {
    if (error) reject(new Error(error.message, { cause: error })); else resolve(stdout.trim());
  }));
}
export interface VerifiedBundle { bytes: Buffer; mcpBytes: Buffer; provenance: PackageProvenance }
export async function verifyPackagedBundle(context: vscode.ExtensionContext): Promise<VerifiedBundle> {
  const manifest: unknown = context.extension.packageJSON;
  if (!record(manifest) || typeof manifest.name !== "string" || typeof manifest.publisher !== "string" || typeof manifest.version !== "string") throw new Error("Dygnosis extension metadata is invalid.");
  const source = await optionalBytes(path.join(context.extensionPath, "SOURCE.json"));
  if (!source) throw new Error("Dygnosis package SOURCE.json is missing. Reinstall the extension.");
  const facts = provenance(JSON.parse(source.toString()), { name: manifest.name, publisher: manifest.publisher });
  if (facts.version !== manifest.version || facts.target !== `${process.platform}-${process.arch}`) throw new Error("Dygnosis package version or native target does not match this extension host. Reinstall the matching package.");
  const binary = path.join(context.extensionPath, "bin", process.platform === "win32" ? "dygnosis.exe" : "dygnosis");
  const bytes = await optionalBytes(binary);
  if (!bytes || digest(bytes) !== facts.binary_sha256) throw new Error("Dygnosis package executable checksum does not match SOURCE.json. Reinstall the extension.");
  if (await executableVersion(binary) !== `dygnosis ${facts.version}`) throw new Error("Dygnosis package executable version does not match SOURCE.json.");
  const mcpBinary = path.join(context.extensionPath, "bin", facts.mcp_binary_name);
  const mcpBytes = await optionalBytes(mcpBinary);
  if (!mcpBytes || mcpBytes.length !== facts.mcp_binary_size || digest(mcpBytes) !== facts.mcp_binary_sha256) throw new Error("Dygnosis MCP executable checksum does not match SOURCE.json. Reinstall the extension.");
  if (await executableVersion(mcpBinary) !== `dygnosis-mcp ${facts.version}`) throw new Error("Dygnosis MCP executable version does not match SOURCE.json.");
  return { bytes, mcpBytes, provenance: facts };
}
export interface ManagedOptions {
  platform?: NodeJS.Platform;
  version?: (file: string) => Promise<string>;
  replace?: (from: string, to: string) => Promise<void>;
  lockAttempts?: number;
  lockDelayMs?: number;
}
export interface ManagedBinary { path: string; version: string; pendingVersion?: string; changed: boolean }
function storagePaths(storage: string, platform: NodeJS.Platform): { directory: string; binary: string; active: string; pending: string; stage: string; lock: string } {
  const directory = path.join(storage, "bin");
  return { directory, binary: path.join(directory, platform === "win32" ? "dygnosis-mcp.exe" : "dygnosis-mcp"), active: path.join(directory, "managed-mcp.json"), pending: path.join(directory, "pending-mcp.json"), stage: path.join(directory, ".dygnosis-mcp.pending"), lock: path.join(directory, ".dygnosis-mcp.lock") };
}
async function storedProvenance(file: string, expected: PackageProvenance): Promise<PackageProvenance | undefined> {
  const bytes = await optionalBytes(file);
  if (!bytes) return undefined;
  const stored = provenance(JSON.parse(bytes.toString()), expected);
  if (stored.target !== expected.target) throw new Error("The managed Dygnosis copy belongs to another native target. Set up MCP on this host again using separate extension storage.");
  return stored;
}
/** Stages only the frozen MCP companion; existing managed launcher paths stay intact. */
export async function ensureManagedBinary(storage: string, bundle: VerifiedBundle, options: ManagedOptions = {}): Promise<ManagedBinary> {
  if (!path.isAbsolute(storage)) throw new Error("Dygnosis extension storage must be an absolute file path.");
  const platform = options.platform ?? process.platform, files = storagePaths(storage, platform);
  const incoming = provenance(bundle.provenance);
  if (!bundle.mcpBytes || digest(bundle.mcpBytes) !== incoming.mcp_binary_sha256 || bundle.mcpBytes.length !== incoming.mcp_binary_size) throw new Error("Verified Dygnosis MCP package bytes changed before installation.");
  await fs.mkdir(files.directory, { recursive: true });
  return withWorkspaceLock(files.lock, async () => {
    let active = await storedProvenance(files.active, incoming);
    let pending = await storedProvenance(files.pending, incoming);
    const working = await optionalBytes(files.binary);
    const workingHash = working && digest(working);
    // Binary replacement is the commit point. A crash before the metadata rename is recoverable.
    if (pending && workingHash === pending.mcp_binary_sha256) {
      await atomicBytes(files.active, Buffer.from(JSON.stringify(pending, null, 2) + "\n"));
      active = pending; pending = undefined;
      await fs.rm(files.pending, { force: true }); await fs.rm(files.stage, { force: true });
    }
    if (working && (!active || workingHash !== active.mcp_binary_sha256)) throw new Error("The managed Dygnosis MCP executable does not match its recorded checksum. It was left intact; reinstall or choose a standalone binary.");
    if (!working && active) throw new Error("The managed Dygnosis MCP executable is missing. Its metadata was left intact; restore extension storage or choose a standalone binary.");
    let desired = incoming, bytes = bundle.mcpBytes;
    if (pending && compareVersions(pending.version, desired.version) > 0) {
      desired = pending;
      const staged = await optionalBytes(files.stage);
      if (!staged || digest(staged) !== pending.mcp_binary_sha256) throw new Error("The pending Dygnosis MCP update is incomplete. The working executable was left intact.");
      bytes = staged;
    }
    if (active) {
      const order = compareVersions(active.version, desired.version);
      if (order === 0 && active.mcp_binary_sha256 !== desired.mcp_binary_sha256) throw new Error(`Dygnosis MCP ${active.version} has different package bytes. The managed copy was left intact.`);
      if (order >= 0) return { path: files.binary, version: active.version, changed: false };
    }
    if (pending && compareVersions(pending.version, desired.version) === 0 && pending.mcp_binary_sha256 !== desired.mcp_binary_sha256) throw new Error(`Dygnosis MCP ${desired.version} has conflicting pending package bytes. The working copy was left intact.`);
    await atomicBytes(files.stage, bytes, platform === "win32" ? 0o600 : 0o755);
    if (digest((await optionalBytes(files.stage)) ?? Buffer.alloc(0)) !== desired.mcp_binary_sha256) throw new Error("Staged Dygnosis MCP checksum verification failed.");
    if (await (options.version ?? executableVersion)(files.stage) !== `dygnosis-mcp ${desired.version}`) throw new Error("Staged Dygnosis MCP executable version verification failed.");
    await atomicBytes(files.pending, Buffer.from(JSON.stringify(desired, null, 2) + "\n"));
    try { await (options.replace ?? fs.rename)(files.stage, files.binary); }
    catch (error) {
      if (platform === "win32" && active && ["EPERM", "EACCES", "EBUSY"].includes(errorCode(error) ?? "")) return { path: files.binary, version: active.version, pendingVersion: desired.version, changed: false };
      throw error;
    }
    await atomicBytes(files.active, Buffer.from(JSON.stringify(desired, null, 2) + "\n"));
    await fs.rm(files.pending, { force: true });
    return { path: files.binary, version: desired.version, changed: true };
  }, options.lockAttempts, options.lockDelayMs);
}

interface JsonNode { start: number; end: number; properties?: Map<string, JsonNode> }
/** JSON.parse validates strict syntax. This walk retains spans, duplicate keys, and untouched bytes. */
function jsonTree(text: string): JsonNode {
  let cursor = 0;
  const white = (): void => { while (/\s/.test(text[cursor] ?? "") && cursor < text.length) ++cursor; };
  const string = (): string => {
    const start = cursor++;
    while (cursor < text.length) { const character = text[cursor++]; if (character === "\\") ++cursor; else if (character === '"') break; }
    return JSON.parse(text.slice(start, cursor)) as string;
  };
  const value = (): JsonNode => {
    white(); const start = cursor;
    if (text[cursor] === "{") {
      ++cursor; white(); const properties = new Map<string, JsonNode>();
      while (text[cursor] !== "}") {
        const key = string(); white(); ++cursor;
        const node = value();
        if (properties.has(key)) throw new Error(`.mcp.json contains duplicate key '${key}'. Resolve it before setup.`);
        properties.set(key, node); white();
        if (text[cursor] !== ",") break; ++cursor; white();
      }
      ++cursor; return { start, end: cursor, properties };
    }
    if (text[cursor] === "[") {
      ++cursor; white(); while (text[cursor] !== "]") { value(); white(); if (text[cursor] !== ",") break; ++cursor; }
      ++cursor;
    } else if (text[cursor] === '"') string();
    else while (cursor < text.length && !/[\s,}\]]/.test(text[cursor])) ++cursor;
    return { start, end: cursor };
  };
  return value();
}
export interface WorkspaceEntry { type: "stdio"; command: string; args: [] }
export interface WorkspaceMerge { text: string; identical: boolean; conflict: boolean }
export function mergeWorkspaceConfig(text: string, command: string): WorkspaceMerge {
  if (!path.isAbsolute(command)) throw new Error("The workspace MCP command must be an absolute executable path.");
  let parsed: unknown;
  try { parsed = JSON.parse(text); } catch (error) { throw new Error(`.mcp.json is not strict JSON: ${error instanceof Error ? error.message : String(error)} The file was left intact.`, { cause: error }); }
  if (!record(parsed) || Array.isArray(parsed)) throw new Error(".mcp.json must contain a JSON object. The file was left intact.");
  const root = jsonTree(text);
  const servers = root.properties?.get("mcpServers");
  if (servers && !servers.properties) throw new Error(".mcp.json mcpServers must be a JSON object. The file was left intact.");
  const entry: WorkspaceEntry = { type: "stdio", command, args: [] };
  const current = servers?.properties?.get("dygnosis");
  if (current) {
    const old: unknown = JSON.parse(text.slice(current.start, current.end));
    if (record(old) && Object.keys(old).length === 3 && old.type === "stdio" && old.command === command && Array.isArray(old.args) && old.args.length === 0) return { text, identical: true, conflict: false };
    return { text: text.slice(0, current.start) + JSON.stringify(entry, null, 2) + text.slice(current.end), identical: false, conflict: true };
  }
  const owner = servers ?? root, properties = owner.properties;
  if (!properties) throw new Error(".mcp.json must contain a JSON object.");
  const last = [...properties.values()].at(-1);
  const offset = last?.end ?? owner.start + 1;
  const property = servers ? `"dygnosis": ${JSON.stringify(entry, null, 2)}` : `"mcpServers": ${JSON.stringify({ dygnosis: entry }, null, 2)}`;
  const insert = `${last ? "," : ""}\n  ${property.replaceAll("\n", "\n  ")}${last ? "" : "\n"}`;
  return { text: text.slice(0, offset) + insert + text.slice(offset), identical: false, conflict: false };
}

export function chooseMcpRoot(folder: vscode.Uri, repositories: readonly { rootUri: vscode.Uri }[]): vscode.Uri {
  const matching = repositories.filter(repository => {
    if (repository.rootUri.scheme !== "file") return false;
    const relative = path.relative(repository.rootUri.fsPath, folder.fsPath);
    return !relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative);
  });
  return matching.sort((a, b) => b.rootUri.fsPath.length - a.rootUri.fsPath.length)[0]?.rootUri ?? folder;
}
interface GitApi { repositories: readonly { rootUri: vscode.Uri }[] }
interface GitExtension { getAPI(version: 1): GitApi }
async function workspaceDestination(): Promise<vscode.Uri | undefined> {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders?.length) { await vscode.window.showInformationMessage("Open a workspace folder before setting up Dygnosis MCP."); return undefined; }
  const chosen = folders.length === 1 ? folders[0] : (await vscode.window.showQuickPick(folders.map(folder => ({ label: folder.name, description: folder.uri.fsPath, folder })), { placeHolder: "Choose a workspace folder for .mcp.json", ignoreFocusOut: true }))?.folder;
  if (!chosen) return undefined;
  if (chosen.uri.scheme !== "file") throw new Error("Dygnosis workspace MCP setup requires a file-backed folder on the extension host.");
  let root = chosen.uri;
  const git = vscode.extensions.getExtension<GitExtension>("vscode.git");
  if (git) {
    try { const api = git.isActive ? git.exports : await git.activate(); root = chooseMcpRoot(chosen.uri, api.getAPI(1).repositories); }
    catch { /* Disabled/unavailable Git: selected folder remains the visible destination. */ }
  }
  return vscode.Uri.joinPath(root, ".mcp.json");
}

interface ConfigSnapshot { disk?: Buffer; document?: vscode.TextDocument; version?: number; source: string; dirty: boolean }
async function configSnapshot(uri: vscode.Uri): Promise<ConfigSnapshot> {
  const disk = await optionalBytes(uri.fsPath);
  let document = vscode.workspace.textDocuments.find(candidate => candidate.uri.toString() === uri.toString());
  if (!document && disk) document = await vscode.workspace.openTextDocument(uri);
  return { disk, document, version: document?.version, source: document?.getText() ?? disk?.toString("utf8") ?? "{}\n", dirty: document?.isDirty ?? false };
}
async function unchangedConfig(uri: vscode.Uri, snapshot: ConfigSnapshot): Promise<void> {
  if (!sameBytes(await optionalBytes(uri.fsPath), snapshot.disk)) throw new Error(".mcp.json changed on disk during setup. Run setup again; no configuration was saved.");
  if (snapshot.document && (snapshot.document.isClosed || snapshot.document.version !== snapshot.version || snapshot.document.getText() !== snapshot.source)) throw new Error(".mcp.json changed during setup. Review the current file and run setup again; no configuration was saved.");
  if (!snapshot.document && vscode.workspace.textDocuments.some(document => document.uri.toString() === uri.toString())) throw new Error(".mcp.json was opened during setup. Run setup again to use its current buffer.");
}
/** Review and persistence are separate so cancellation and revision races are checked before mutation. */
export async function writeWorkspaceConfig(uri: vscode.Uri, command: string, review: (before: string, after: string) => Promise<boolean>): Promise<"saved" | "identical" | "cancelled"> {
  const snapshot = await configSnapshot(uri);
  let merged: WorkspaceMerge;
  try { merged = mergeWorkspaceConfig(snapshot.source, command); }
  catch (error) { throw new Error(`${uri.fsPath}: ${error instanceof Error ? error.message : String(error)}`, { cause: error }); }
  if (merged.identical) { if (snapshot.document) await vscode.window.showTextDocument(snapshot.document); return "identical"; }
  if ((merged.conflict || snapshot.dirty) && !await review(snapshot.source, merged.text)) return "cancelled";
  return withWorkspaceLock(`${uri.fsPath}.dygnosis.lock`, async () => {
    await unchangedConfig(uri, snapshot);
    if (snapshot.document) {
      const document = snapshot.document;
      const edit = new vscode.WorkspaceEdit();
      edit.replace(uri, new vscode.Range(document.positionAt(0), document.positionAt(snapshot.source.length)), merged.text);
      if (!await vscode.workspace.applyEdit(edit)) throw new Error("VS Code could not apply the reviewed .mcp.json edit. Run setup again.");
      const unchangedDisk = sameBytes(await optionalBytes(uri.fsPath), snapshot.disk);
      if (!unchangedDisk || document.isClosed || document.getText() !== merged.text || document.version !== (snapshot.version ?? 0) + 1) throw new Error(".mcp.json changed while applying setup. Its buffer was left for review; no configuration was saved.");
      if (!await document.save()) throw new Error("VS Code could not save the reviewed .mcp.json edit. Its buffer was left for review.");
      if (document.getText() !== merged.text || (await optionalBytes(uri.fsPath))?.toString("utf8") !== merged.text) throw new Error(".mcp.json changed during save. Review the file before using the MCP configuration.");
    } else {
      try {
        // Same-directory hard-link creation is atomic and fails if another writer created the destination.
        await atomicBytes(uri.fsPath, Buffer.from(merged.text), 0o600, () => unchangedConfig(uri, snapshot), fs.link);
      } catch (error) {
        if (errorCode(error) === "EEXIST") throw new Error(".mcp.json was created during setup. Review the current file and run setup again; no configuration was saved.", { cause: error });
        throw error;
      }
    }
    const result = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(result);
    return "saved" as const;
  });
}

/** Registers independently of the language client; activation never edits workspace configuration. */
export function registerWorkspaceMcp(context: vscode.ExtensionContext, service?: { log: Log }): vscode.Disposable {
  const log = service?.log ?? (() => {});
  const reviews = new Map<string, string>();
  const provider = vscode.workspace.registerTextDocumentContentProvider(reviewScheme, { provideTextDocumentContent: uri => reviews.get(uri.toString()) ?? "" });
  let disposed = false, running = false, retry: NodeJS.Timeout | undefined, retries = 0, pendingShown = false;
  const storage = (): string | undefined => {
    const uri = context.globalStorageUri;
    // Desktop VS Code can expose its disk-backed user data through this scheme.
    if (uri.scheme !== "file" && uri.scheme !== "vscode-userdata") return undefined;
    const directory = uri.with({ scheme: "file" }).fsPath;
    return path.isAbsolute(directory) ? directory : undefined;
  };
  const reportManaged = (binary: ManagedBinary): void => {
    if (binary.pendingVersion) {
      log(`Workspace MCP update pending: ${binary.version} -> ${binary.pendingVersion}. Stop the other client's Dygnosis MCP server so its locked executable can be replaced, then restart it.`);
      if (!pendingShown && !disposed) {
        pendingShown = true;
        void vscode.window.showWarningMessage(`Dygnosis MCP ${binary.pendingVersion} is pending. Stop the other client's Dygnosis MCP server so the working ${binary.version} can be replaced, then restart it. Setup will retry while this window is active and on the next activation.`);
      }
      if (!disposed && retries < 5 && !retry) { ++retries; retry = setTimeout(() => { retry = undefined; void maintain(); }, 15000); }
    } else if (binary.changed) log(`Workspace MCP companion updated to ${binary.version}: ${binary.path}. Restart other clients' MCP server to load it; .mcp.json does not change.`);
  };
  const maintain = async (): Promise<void> => {
    try {
      const directory = storage();
      if (disposed || !directory) return;
      const files = storagePaths(directory, process.platform);
      if (!await optionalBytes(files.active) && !await optionalBytes(files.pending)) return;
      const bundle = await verifyPackagedBundle(context);
      if (disposed) return;
      reportManaged(await ensureManagedBinary(directory, bundle));
    } catch (error) { log(`Workspace MCP managed update: ${String(error)}`); }
  };
  const review = async (uri: vscode.Uri, before: string, after: string): Promise<boolean> => {
    const id = randomUUID(), old = vscode.Uri.parse(`${reviewScheme}:/${id}/before.json`), proposed = vscode.Uri.parse(`${reviewScheme}:/${id}/proposed.json`);
    reviews.set(old.toString(), before); reviews.set(proposed.toString(), after);
    await vscode.commands.executeCommand("vscode.diff", old, proposed, `Dygnosis MCP setup: ${uri.fsPath}`, { preview: true });
    return await vscode.window.showWarningMessage(`Review the proposed .mcp.json change in ${uri.fsPath}. Save this result?`, { modal: true }, "Save reviewed setup") === "Save reviewed setup" && !disposed;
  };
  const command = vscode.commands.registerCommand(commandId, async () => {
    if (running) { await vscode.window.showInformationMessage("Dygnosis workspace MCP setup is already running in this window."); return; }
    running = true;
    try {
      if (!vscode.workspace.isTrusted) throw new Error("Trust this workspace before setting up Dygnosis workspace MCP.");
      const uri = await workspaceDestination();
      if (!uri || disposed) return;
      log(`Workspace MCP destination: ${uri.fsPath}`);
      const selected = await resolveMcpBinary(context, log);
      let executable = selected.path;
      if (selected.override) {
        await validateMcpBinary(selected, log);
        log(`Workspace MCP uses user-managed dynare.serverPath: ${selected.path}. Updates are your responsibility.`);
      } else {
        const directory = storage();
        if (!directory) throw new Error("Dygnosis workspace MCP requires file-backed extension storage on this host.");
        const managed = await ensureManagedBinary(directory, await verifyPackagedBundle(context));
        executable = managed.path; reportManaged(managed);
      }
      if (disposed) return;
      const result = await writeWorkspaceConfig(uri, executable, (before, after) => review(uri, before, after));
      if (result !== "cancelled") await vscode.window.showInformationMessage(`Dygnosis MCP ${result === "identical" ? "is already configured" : "configuration saved"}: ${uri.fsPath}. ${result === "identical" ? "No configuration change was needed." : "Reconnect the agent's MCP server to load it."}`);
    } catch (error) { log(`Workspace MCP setup: ${String(error)}`); if (!disposed) await vscode.window.showErrorMessage(`Dygnosis workspace MCP setup: ${error instanceof Error ? error.message : String(error)}`); }
    finally { running = false; }
  });
  const closed = vscode.workspace.onDidCloseTextDocument(document => { if (document.uri.scheme === reviewScheme) reviews.delete(document.uri.toString()); });
  void maintain();
  return vscode.Disposable.from(command, provider, closed, new vscode.Disposable(() => { disposed = true; if (retry) clearTimeout(retry); reviews.clear(); }));
}
