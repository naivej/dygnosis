import { refuse, runCommand } from "../client/refusal";
import * as path from "node:path";
import * as vscode from "vscode";
import { record } from "../client/protocol";
import { workspaceDiagnosticsSetting } from "../client/settings";
import type { ModelSnapshot } from "../client/client";

const states = ["pending", "checking", "checked", "incomplete", "failed", "excluded"] as const;
type RootState = typeof states[number];
type DiscoveryState = "pending" | "discovering" | "complete" | "cancelled" | "disabled";
export interface WorkspaceRoot {
  root_uri: string; state: RootState; revision: string | null;
  errors: number; warnings: number; failure: string | null; dependency_candidates: string[];
}
export interface WorkspaceStatus {
  schema_version: 1; pass_revision: number; enabled: boolean; discovery: DiscoveryState;
  cancelled: boolean; complete: boolean; coverage_complete: boolean;
  counts: Record<RootState, number>; roots: WorkspaceRoot[];
  discovery_failures: { folder_uri: string; failure: string }[];
  include_files?: string[];
  metrics: { discovery_ms: number; analysis_ms: number; completed_jobs: number; reused_jobs: number; elapsed_ms: number | null };
}
export interface WorkspaceCapability {
  schema_version: 1; status_command: "dynare/workspaceStatus"; recheck_command: "dynare/recheckWorkspace";
  cancel_command: "dynare/cancelWorkspace"; active_model_notification: "dynare/activeModelChanged";
  status_notification: "dynare/workspaceStatusChanged";
}
interface WorkspaceConnection {
  readonly initializeResult?: unknown;
  onNotification(method: string, handler: (value: unknown) => void): vscode.Disposable;
  sendNotification(method: string, params: unknown): Promise<void>;
  isRunning?(): boolean;
}
/** Root selection is a synchronous read of proven context, never owner discovery. */
export interface WorkspaceClientPort {
  readonly client: WorkspaceConnection | undefined; readonly currentInstance: number;
  readonly onDidChange: vscode.Event<void>; readonly onDidReady?: vscode.Event<void>;
  readonly onDidUpdateModelInfo: vscode.Event<ModelSnapshot>;
  ensureStarted(): Promise<void>;
  execute(command: string, args: unknown[], token?: vscode.CancellationToken): Promise<unknown>;
  chosenRootForDocument(document: vscode.TextDocument): vscode.Uri | undefined;
  log(message: string): void;
}
function integer(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value >= 0; }
function duration(value: unknown): value is number { return typeof value === "number" && Number.isFinite(value) && value >= 0; }
function fileUri(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try { const uri = new URL(value); return uri.protocol === "file:" && uri.pathname.startsWith("/") && !uri.search && !uri.hash; } catch { return false; }
}
function identity(uri: vscode.Uri): string {
  return uri.scheme === "file" && process.platform === "win32" ? uri.fsPath.toLowerCase().replace(/[\\/]+$/, "") : uri.toString().replace(/\/+$/, "");
}
function nullableText(value: unknown): value is string | null { return value === null || typeof value === "string"; }
function diagnostics(count: number, noun: "Error" | "Warning"): string { return `${count} ${noun}${count === 1 ? "" : "s"}`; }
/** All required fields are checked; future additive fields are harmless. */
export function parseWorkspaceStatus(value: unknown): WorkspaceStatus {
  const invalid = (): never => { throw new Error("Unsupported workspace status. Use the bundled binary or update dynare.serverPath."); };
  if (!record(value) || value.schema_version !== 1 || !integer(value.pass_revision) ||
      typeof value.enabled !== "boolean" || typeof value.cancelled !== "boolean" || typeof value.complete !== "boolean" ||
      typeof value.coverage_complete !== "boolean" || typeof value.discovery !== "string" || !["pending", "discovering", "complete", "cancelled", "disabled"].includes(value.discovery) ||
      !record(value.counts) || !Array.isArray(value.roots) || !Array.isArray(value.discovery_failures) || !record(value.metrics)) return invalid();
  const counts = {} as Record<RootState, number>;
  for (const state of states) { const count = value.counts[state]; if (!integer(count)) return invalid(); counts[state] = count; }
  const rootKeys = new Set<string>();
  const roots = value.roots.map((root: unknown): WorkspaceRoot => {
    if (!record(root) || !fileUri(root.root_uri) || !states.includes(root.state as RootState) || !nullableText(root.revision) ||
        !integer(root.errors) || !integer(root.warnings) || !nullableText(root.failure) || !Array.isArray(root.dependency_candidates) ||
        !root.dependency_candidates.every(fileUri)) return invalid();
    const key = identity(vscode.Uri.parse(root.root_uri));
    if (rootKeys.has(key)) return invalid(); rootKeys.add(key);
    return { root_uri: root.root_uri, state: root.state as RootState, revision: root.revision, errors: root.errors,
      warnings: root.warnings, failure: root.failure, dependency_candidates: root.dependency_candidates };
  });
  if (states.some(state => roots.filter(root => root.state === state).length !== counts[state])) return invalid();
  const failures = value.discovery_failures.map((failure: unknown) => {
    if (!record(failure) || !fileUri(failure.folder_uri) || typeof failure.failure !== "string") return invalid();
    return { folder_uri: failure.folder_uri, failure: failure.failure };
  });
  let includeFiles: string[] | undefined;
  if (value.include_files !== undefined) {
    if (!Array.isArray(value.include_files)) return invalid();
    const keys = new Set<string>();
    includeFiles = value.include_files.map((file: unknown) => {
      if (!fileUri(file) || !/\.inc$/i.test(vscode.Uri.parse(file).path)) return invalid();
      const key = identity(vscode.Uri.parse(file));
      if (keys.has(key)) return invalid(); keys.add(key); return file;
    });
  }
  const metrics = value.metrics;
  if (!duration(metrics.discovery_ms) || !duration(metrics.analysis_ms) || !integer(metrics.completed_jobs) ||
      !integer(metrics.reused_jobs) || (metrics.elapsed_ms !== null && !duration(metrics.elapsed_ms))) return invalid();
  if ((!value.enabled && (value.discovery !== "disabled" || value.complete || value.coverage_complete)) ||
      (value.complete && (value.cancelled || value.discovery !== "complete" || counts.pending > 0 || counts.checking > 0)) ||
      (value.coverage_complete && (!value.complete || failures.length > 0 || counts.failed > 0 || counts.incomplete > 0))) return invalid();
  return { schema_version: 1, pass_revision: value.pass_revision, enabled: value.enabled, discovery: value.discovery as DiscoveryState,
    cancelled: value.cancelled, complete: value.complete, coverage_complete: value.coverage_complete, counts, roots, include_files: includeFiles, discovery_failures: failures,
    metrics: { discovery_ms: metrics.discovery_ms, analysis_ms: metrics.analysis_ms, completed_jobs: metrics.completed_jobs,
      reused_jobs: metrics.reused_jobs, elapsed_ms: metrics.elapsed_ms } };
}
export function parseWorkspaceCapability(initializeResult: unknown): WorkspaceCapability | undefined {
  if (!record(initializeResult) || !record(initializeResult.capabilities)) return undefined;
  const capabilities = initializeResult.capabilities, experimental = capabilities.experimental;
  if (!record(experimental) || !record(experimental.dygnosis) || !record(experimental.dygnosis.workspaceDiagnostics)) return undefined;
  const capability = experimental.dygnosis.workspaceDiagnostics;
  const expected: WorkspaceCapability = { schema_version: 1, status_command: "dynare/workspaceStatus", recheck_command: "dynare/recheckWorkspace",
    cancel_command: "dynare/cancelWorkspace", active_model_notification: "dynare/activeModelChanged", status_notification: "dynare/workspaceStatusChanged" };
  const commands = record(capabilities.executeCommandProvider) ? capabilities.executeCommandProvider.commands : undefined;
  if (Object.entries(expected).some(([key, value]) => capability[key] !== value) || !Array.isArray(commands) ||
      [expected.status_command, expected.recheck_command, expected.cancel_command].some(command => !commands.includes(command))) return undefined;
  return expected;
}
interface StatusContent { text: string; tooltip: vscode.MarkdownString; accessibilityLabel: string }
export function workspaceStatusContent(status: WorkspaceStatus): StatusContent {
  const counts = status.counts, selected = status.roots.length - counts.excluded;
  const finished = counts.checked + counts.incomplete + counts.failed;
  let label: string, icon: string;
  if (!status.enabled) { label = "off"; icon = "circle-slash"; }
  else if (status.cancelled) { label = `cancelled · ${counts.checked}/${selected} checked`; icon = "debug-pause"; }
  else if (status.discovery === "pending" || status.discovery === "discovering") { label = "discovering"; icon = "sync~spin"; }
  else if (!status.complete) { label = `${finished}/${selected} finished`; icon = "sync~spin"; }
  else if (!status.coverage_complete) { label = `${counts.checked}/${selected} checked`; icon = "warning"; }
  else if (!selected) { label = status.roots.length ? "all root models excluded" : "no root models"; icon = "info"; }
  else { label = `${counts.checked}/${selected} checked`; icon = "check"; }
  const rows: [string, number][] = [["Checked", counts.checked]];
  const notFullyChecked = counts.incomplete + counts.failed;
  if (notFullyChecked) rows.push(["Not fully checked", notFullyChecked]);
  if (counts.excluded) rows.push(["Excluded", counts.excluded]);
  const notes: string[] = [];
  if (!status.enabled) notes.push("Workspace diagnostics are off. Enable dynare.workspaceDiagnostics in Settings.");
  else {
    if (status.cancelled) {
      const remaining = counts.pending + counts.checking;
      notes.push(`Cancelled.${remaining ? ` ${remaining} model${remaining === 1 ? " was" : "s were"} not checked.` : ""} Completed results remain. Checks resume after a file edit/change or Recheck.`);
    }
    const failures = status.discovery_failures.length;
    if (failures) notes.push(`Could not read ${failures} folder${failures === 1 ? "" : "s"}. The model list may be incomplete.`);
  }
  const tooltip = new vscode.MarkdownString("**Models in workspace**\n\n");
  if (status.enabled) tooltip.appendMarkdown(["| Models | Count |", "|:---|---:|", ...rows.map(([name, count]) => `| ${name} | ${count} |`)].join("\n"));
  for (const note of notes) tooltip.appendMarkdown("\n\n").appendText(note);
  return { text: `$(${icon}) Dynare workspace: ${label}`, tooltip,
    accessibilityLabel: [`Dynare workspace: ${label}.`, ...(status.enabled ? rows.map(([name, count]) => `${name}: ${count}.`) : []), ...notes, "Open workspace checks."].join(" ") };
}
class WorkspaceItem extends vscode.TreeItem {
  declare label: string;
  constructor(id: string, label: string, icon: string, readonly children: WorkspaceItem[] = [], readonly folder?: vscode.WorkspaceFolder) {
    super(label, children.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
    this.id = id; this.iconPath = new vscode.ThemeIcon(icon);
    this.accessibilityInformation = { label };
  }
}
class WorkspaceTree implements vscode.TreeDataProvider<WorkspaceItem>, vscode.Disposable {
  private readonly changed = new vscode.EventEmitter<WorkspaceItem | undefined>();
  readonly onDidChangeTreeData = this.changed.event;
  private rows: WorkspaceItem[] = [];
  getTreeItem(item: WorkspaceItem): vscode.TreeItem { return item; }
  getChildren(item?: WorkspaceItem): WorkspaceItem[] { return item?.children ?? this.rows; }
  replace(rows: WorkspaceItem[]): void { this.rows = rows; this.changed.fire(undefined); }
  dispose(): void { this.rows = []; this.changed.dispose(); }
}
function folders(): readonly vscode.WorkspaceFolder[] { return (vscode.workspace.workspaceFolders ?? []).filter(folder => folder.uri.scheme === "file"); }
function containingFolder(uri: vscode.Uri, candidates: readonly vscode.WorkspaceFolder[]): vscode.WorkspaceFolder | undefined {
  const key = identity(uri);
  return candidates.filter(folder => { const base = identity(folder.uri).replace(/[\\/]$/, ""); return key === base || key.startsWith(`${base}${uri.scheme === "file" && process.platform === "win32" ? path.sep : "/"}`); })
    .sort((left, right) => right.uri.path.length - left.uri.path.length)[0];
}
function treeRows(status: WorkspaceStatus, currentFolders: readonly vscode.WorkspaceFolder[], collapsed: ReadonlySet<string>): WorkspaceItem[] {
  return currentFolders.map(folder => {
    const row = new WorkspaceItem(identity(folder.uri), folder.name, "folder", [], folder);
    row.contextValue = "dygnosis.workspaceFolder"; row.resourceUri = folder.uri;
    const directories = new Map<string, WorkspaceItem>([[identity(folder.uri), row]]);
    const directory = (uri: vscode.Uri): WorkspaceItem => {
      const key = identity(uri), existing = directories.get(key);
      if (existing) return existing;
      const item = new WorkspaceItem(key, path.basename(uri.fsPath), "folder", [], folder);
      item.contextValue = "dygnosis.workspaceDirectory";
      item.resourceUri = uri; directories.set(key, item);
      directory(vscode.Uri.file(path.dirname(uri.fsPath))).children.push(item);
      return item;
    };
    const addFile = (uri: vscode.Uri, item: WorkspaceItem): void => {
      directory(vscode.Uri.file(path.dirname(uri.fsPath))).children.push(item);
    };
    const roots = status.roots.filter(root => containingFolder(vscode.Uri.parse(root.root_uri), currentFolders) === folder);
    for (const root of roots) {
      const uri = vscode.Uri.parse(root.root_uri), label = path.basename(uri.fsPath);
      const detail = `${root.state === "checked" ? "Checked" : root.state[0].toUpperCase() + root.state.slice(1)}${root.state === "checked" ? ` · ${diagnostics(root.errors, "Error")}, ${diagnostics(root.warnings, "Warning")}` : ""}`;
      const item = new WorkspaceItem(identity(uri), label, root.state === "failed" || root.state === "incomplete" ? "warning" :
        root.state === "checked" ? root.errors ? "error" : "check" : root.state === "checking" ? "sync~spin" : root.state === "excluded" ? "circle-slash" : "clock");
      item.resourceUri = uri; item.description = detail;
      item.tooltip = `${uri.fsPath}\n${detail}${root.failure ? `\n${root.failure}` : ""}`;
      item.accessibilityInformation = { label: `${label}. ${detail}` };
      item.command = { command: "vscode.open", title: "Open model", arguments: [uri] };
      addFile(uri, item);
    }
    for (const file of status.include_files ?? []) {
      const uri = vscode.Uri.parse(file);
      if (containingFolder(uri, currentFolders) !== folder) continue;
      const item = new WorkspaceItem(identity(uri), path.basename(uri.fsPath), "file-code");
      item.resourceUri = uri; item.description = "Include file";
      item.tooltip = `${uri.fsPath}\nInclude file`;
      item.accessibilityInformation = { label: `${item.label}. Include file` };
      item.command = { command: "vscode.open", title: "Open file", arguments: [uri] };
      addFile(uri, item);
    }
    const failures = status.discovery_failures.filter(failure => identity(vscode.Uri.parse(failure.folder_uri)) === identity(folder.uri)).map((failure, index) => {
      const item = new WorkspaceItem(`${folder.uri.toString()}:failure:${index}`, "Discovery failed", "warning");
      item.description = failure.failure; item.tooltip = failure.failure;
      item.accessibilityInformation = { label: `Discovery failed. ${failure.failure}` }; return item;
    });
    for (const item of directories.values()) {
      item.children.sort((left, right) => Number(!directories.has(left.id!)) - Number(!directories.has(right.id!)) ||
        left.label.localeCompare(right.label));
      item.collapsibleState = collapsed.has(item.id!) ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.Expanded;
    }
    if (!roots.length && !failures.length) row.children.push(new WorkspaceItem(`${folder.uri.toString()}:empty`,
      status.discovery === "pending" || status.discovery === "discovering" ? "Discovering root models…" :
        status.discovery === "cancelled" ? "Discovery cancelled" : "No root .mod or .dyn models", "info"));
    row.children.unshift(...failures);
    return row;
  });
}

/** Presentation and watcher adapter only; analysis stays in the server. */
export function registerWorkspaceStatus(service: WorkspaceClientPort): vscode.Disposable {
  const item = vscode.window.createStatusBarItem("dygnosis.workspaceCoverage", vscode.StatusBarAlignment.Left, 39);
  item.name = "Dynare workspace coverage"; item.command = "dygnosis.workspaceStatus";
  const tree = new WorkspaceTree(), view = vscode.window.createTreeView("dygnosis.workspace", { treeDataProvider: tree, showCollapseAll: true });
  const collapsed = new Set<string>();
  let disposed = false, connection: WorkspaceConnection | undefined, instance = -1;
  let capability: WorkspaceCapability | undefined, status: WorkspaceStatus | undefined, generation = 0, response = 0, minPass = 0;
  let barrier = false, activeRoot: string | null | undefined;
  let bufferedNotification: { value: WorkspaceStatus; client: WorkspaceConnection; instance: number; generation: number; settings: string } | undefined;
  let startupFailed = false;
  let request: vscode.CancellationTokenSource | undefined, notification: vscode.Disposable | undefined;
  let inputTimer: ReturnType<typeof setTimeout> | undefined;
  const watchers = new Map<string, vscode.Disposable>();
  const dependencies = new Map<string, vscode.Disposable>();
  const dependencyTargets = new Map<string, Set<string>>();
  const enabled = (): boolean => workspaceDiagnosticsSetting(message => service.log(message));
  const snapshot = (): string => JSON.stringify([enabled(), folders().map(folder => [folder.uri.toString(),
    vscode.workspace.getConfiguration("dynare", folder.uri).get<unknown>("workspaceExcludePaths", [])])]);
  const clearWatchers = (owned: Map<string, vscode.Disposable>): void => { for (const watcher of owned.values()) watcher.dispose(); owned.clear(); };
  const cancelRequest = (): void => { request?.cancel(); request?.dispose(); request = undefined; };
  const beginBarrier = (): void => { barrier = true; bufferedNotification = undefined; };
  const message = (label: string, icon = "info", detail = label): void => {
    item.text = `$(${icon}) Dynare workspace: ${label}`;
    item.tooltip = new vscode.MarkdownString("**Models in workspace**\n\n").appendText(detail);
    item.accessibilityInformation = { label: `Dynare workspace: ${label}. Open workspace checks.` };
    item.show(); view.message = detail; view.description = undefined;
    const help = new WorkspaceItem("help", "Open Dygnosis Help", "question");
    help.command = { command: "dygnosis.openHelp", title: "Open Dygnosis Help", arguments: [label === "unavailable" ? "troubleshoot" : "workspace-checks"] };
    tree.replace([help]);
  };
  const render = (): void => {
    if (disposed) return;
    if (!enabled()) { message("off", "circle-slash", "Workspace diagnostics are off. Enable dynare.workspaceDiagnostics in Settings."); return; }
    if (!folders().length) {
      message("no folders", "info", "Open a file-backed workspace folder to list Dynare files and check saved .mod and .dyn models.");
      view.message = undefined; tree.replace([]); return;
    }
    if (!status) { message(capability ? "updating" : connection || startupFailed ? "unavailable" : "starting", capability || (!connection && !startupFailed) ? "sync~spin" : "warning",
      capability ? "Refreshing workspace coverage." : connection ? "Workspace checks require a compatible engine. Use the bundled binary or update dynare.serverPath." :
        startupFailed ? "Dygnosis is unavailable. Restart the language server or open Dygnosis Output for details." : "Starting Dygnosis workspace checks."); return; }
    const content = workspaceStatusContent(status); item.text = content.text; item.tooltip = content.tooltip;
    item.accessibilityInformation = { label: content.accessibilityLabel }; item.show();
    const outcome = status.cancelled ? "Cancelled until a file edit/change or Recheck." :
      status.discovery_failures.length ? "Coverage and the file list are incomplete: folder discovery failed. Previous entries remain visible for folders that could not be read." :
      status.counts.failed || status.counts.incomplete ? "Coverage is incomplete. Open affected models for details." : undefined;
    view.message = [outcome, status.include_files === undefined ? "Include files are not listed by this engine. Use the bundled binary or update dynare.serverPath." : undefined].filter(Boolean).join("\n") || undefined;
    view.description = `${status.counts.checked} checked`;
    const rows = treeRows(status, folders(), collapsed);
    if (status.discovery_failures.length || status.counts.failed || status.counts.incomplete || status.roots.every(root => root.state === "excluded")) {
      const help = new WorkspaceItem("help", "Open Dygnosis Help", "question");
      help.command = { command: "dygnosis.openHelp", title: "Open Dygnosis Help", arguments: ["workspace-checks"] };
      rows.push(help);
    }
    tree.replace(rows);
  };
  const current = (client: WorkspaceConnection, expectedInstance: number, expectedGeneration: number, expectedSnapshot: string): boolean =>
    !disposed && service.client === client && service.currentInstance === expectedInstance && connection === client &&
    generation === expectedGeneration && snapshot() === expectedSnapshot && (client.isRunning?.() ?? true);
  const refresh = async (command?: string): Promise<void> => {
    if (!connection && (!enabled() || !folders().length)) return;
    try { await service.ensureStarted(); }
    catch (error) {
      if (disposed) return;
      if (command) throw error;
      startupFailed = true; service.log(String(error)); render(); return;
    }
    synchronize();
    const client = connection, supported = capability;
    if (disposed) return;
    if (!client || !supported) {
      if (command) refuse("Workspace checks are unavailable. Use an engine that supports workspace checks.");
      return;
    }
    cancelRequest(); const cancellation = new vscode.CancellationTokenSource(); request = cancellation;
    const expectedGeneration = generation, expectedInstance = instance, expectedSnapshot = snapshot(), serial = ++response;
    try {
      const value = await service.execute(command ?? supported.status_command, [], cancellation.token);
      if (cancellation.token.isCancellationRequested || serial !== response || !current(client, expectedInstance, expectedGeneration, expectedSnapshot)) return;
      const next = parseWorkspaceStatus(value);
      const buffered = bufferedNotification; bufferedNotification = undefined; barrier = false;
      let latest = next.pass_revision >= minPass && next.enabled === enabled() ? next : undefined;
      // A command/query can snapshot pending work before a notification reports
      // its completion. Keep that notification, but require this same input
      // epoch and at least the acknowledged pass. Cancel does not advance the
      // pass, so its reply defeats a same-pass notification from before Cancel.
      if (buffered && current(buffered.client, buffered.instance, buffered.generation, buffered.settings) &&
          buffered.value.pass_revision >= Math.max(minPass, next.pass_revision) && buffered.value.enabled === enabled() &&
          (command !== supported.cancel_command || buffered.value.pass_revision > next.pass_revision || buffered.value.cancelled || !next.cancelled)) latest = buffered.value;
      if (!latest) return;
      minPass = latest.pass_revision; status = latest; updateDependencyWatchers(); sendActiveRoot(); render();
    } catch (error) {
      if (!cancellation.token.isCancellationRequested && serial === response && current(client, expectedInstance, expectedGeneration, expectedSnapshot)) {
        barrier = false; bufferedNotification = undefined; status = undefined; clearWatchers(dependencies);
        if (command) { render(); throw error; }
        service.log(String(error));
        message("unavailable", "warning", String(error));
      }
    } finally { if (request === cancellation) request = undefined; cancellation.dispose(); }
  };
  const changedInputs = (): void => {
    ++generation; ++response; cancelRequest(); beginBarrier();
    status = undefined; render(); void refresh();
  };
  const documentChanged = (document: vscode.TextDocument): void => {
    if (!enabled() || !capability || document.languageId !== "dynare" || !["file", "untitled"].includes(document.uri.scheme)) return;
    if (status?.complete || status?.cancelled) minPass = status.pass_revision + 1;
    ++generation; ++response; cancelRequest(); beginBarrier(); status = undefined; render();
    if (inputTimer) clearTimeout(inputTimer);
    inputTimer = setTimeout(() => { inputTimer = undefined; void refresh(); }, 75);
  };
  const watch = (pattern: vscode.GlobPattern, accepts: (uri: vscode.Uri) => boolean): vscode.Disposable => {
    const watcher = vscode.workspace.createFileSystemWatcher(pattern), client = connection, expectedInstance = instance;
    const subscriptions: vscode.Disposable[] = [watcher];
    for (const [event, type] of [[watcher.onDidCreate, 1], [watcher.onDidChange, 2], [watcher.onDidDelete, 3]] as const) {
      subscriptions.push(event(uri => {
        if (disposed || !enabled() || !client || connection !== client || service.client !== client || service.currentInstance !== expectedInstance || !accepts(uri)) return;
        if (status?.complete || status?.cancelled) minPass = status.pass_revision + 1;
        ++generation; ++response; cancelRequest(); beginBarrier(); status = undefined; render();
        void client.sendNotification("workspace/didChangeWatchedFiles", { changes: [{ uri: uri.toString(), type }] })
          .then(() => refresh()).catch((error: unknown) => service.log(String(error)));
      }));
    }
    return vscode.Disposable.from(...subscriptions);
  };
  function updateDependencyWatchers(): void {
    if (!enabled() || !status || !connection) { clearWatchers(dependencies); dependencyTargets.clear(); return; }
    const groups = new Map<string, { directory: vscode.Uri; targets: Set<string> }>();
    for (const root of status.roots) {
      if (root.state === "excluded" || !containingFolder(vscode.Uri.parse(root.root_uri), folders())) continue;
      for (const candidate of root.dependency_candidates) {
        const uri = vscode.Uri.parse(candidate), directory = vscode.Uri.file(path.dirname(uri.fsPath)), key = identity(directory);
        const group = groups.get(key) ?? { directory, targets: new Set<string>() }; group.targets.add(identity(uri)); groups.set(key, group);
      }
    }
    for (const [key, watcher] of dependencies) if (!groups.has(key)) { watcher.dispose(); dependencies.delete(key); dependencyTargets.delete(key); }
    for (const [key, group] of groups) {
      dependencyTargets.set(key, group.targets);
      if (!dependencies.has(key)) dependencies.set(key, watch(new vscode.RelativePattern(group.directory, "*"), uri =>
        !watchedWorkspaceFile(uri) && (dependencyTargets.get(key)?.has(identity(uri)) ?? false)));
    }
  }
  const updateRootWatchers = (): void => {
    clearWatchers(watchers);
    if (!enabled() || !capability || !connection) return;
    for (const folder of folders()) watchers.set(folder.uri.toString(), watch(new vscode.RelativePattern(folder.uri,
      "**/*.{[mM][oO][dD],[dD][yY][nN],[iI][nN][cC]}"), watchedWorkspaceFile));
  };
  function watchedWorkspaceFile(uri: vscode.Uri): boolean {
    const folder = containingFolder(uri, folders());
    return !!folder && /\.(mod|dyn|inc)$/i.test(uri.path) &&
      !path.relative(folder.uri.fsPath, uri.fsPath).split(path.sep).slice(0, -1).some(part => part.startsWith("+"));
  }
  const sendActiveRoot = (): void => {
    if (!connection || !capability || !(connection.isRunning?.() ?? true)) return;
    const document = vscode.window.activeTextEditor?.document;
    const root = document && !document.isClosed ? service.chosenRootForDocument(document)?.toString() ?? null : null;
    if (root === activeRoot) return; activeRoot = root;
    const client = connection;
    void client.sendNotification(capability.active_model_notification, { root_uri: root }).catch((error: unknown) => {
      if (connection === client) { activeRoot = undefined; service.log(String(error)); }
    });
  };
  function synchronize(): void {
    if (disposed) return;
    const client = service.client;
    if (connection === client && instance === service.currentInstance && (client?.isRunning?.() ?? true)) { sendActiveRoot(); return; }
    startupFailed = client && (client.isRunning?.() ?? true) ? false : startupFailed || !!connection;
    ++generation; ++response; cancelRequest(); notification?.dispose(); notification = undefined;
    clearWatchers(watchers); clearWatchers(dependencies); status = undefined; minPass = 0; barrier = false; bufferedNotification = undefined; activeRoot = undefined;
    dependencyTargets.clear();
    connection = client?.isRunning?.() === false ? undefined : client; instance = service.currentInstance;
    capability = parseWorkspaceCapability(connection?.initializeResult);
    if (connection && capability) {
      const subscribedClient = connection, subscribedInstance = instance;
      notification = connection.onNotification(capability.status_notification, (value: unknown) => {
        if (disposed || connection !== subscribedClient || service.client !== subscribedClient || instance !== subscribedInstance ||
            service.currentInstance !== subscribedInstance || !(subscribedClient.isRunning?.() ?? true)) return;
        try {
          const next = parseWorkspaceStatus(value);
          if (next.pass_revision < minPass || next.enabled !== enabled()) return;
          if (barrier) {
            if (bufferedNotification?.value.pass_revision === next.pass_revision && bufferedNotification.value.cancelled && !next.cancelled) return;
            if (!bufferedNotification || next.pass_revision >= bufferedNotification.value.pass_revision)
              bufferedNotification = { value: next, client: subscribedClient, instance: subscribedInstance, generation, settings: snapshot() };
            return;
          }
          ++response; cancelRequest(); minPass = next.pass_revision; status = next; updateDependencyWatchers(); sendActiveRoot(); render();
        } catch (error) { bufferedNotification = undefined; status = undefined; clearWatchers(dependencies); service.log(String(error)); message("unavailable", "warning", String(error)); }
      });
      updateRootWatchers(); sendActiveRoot(); void refresh();
    }
    render();
  }
  const startFolders = (): void => {
    render(); if (enabled() && folders().length) void service.ensureStarted().then(() => {
      if (disposed) return; startupFailed = !service.client; synchronize(); render();
    }).catch((error: unknown) => {
      if (!disposed) { startupFailed = true; service.log(String(error)); message("unavailable", "warning", String(error)); }
    });
  };
  const reconfigure = (): void => {
    if (status) minPass = status.pass_revision + 1;
    clearWatchers(dependencies); updateRootWatchers(); changedInputs(); startFolders();
  };
  const configureExclusions = async (target?: unknown): Promise<void> => {
    const currentFolders = folders();
    if (!currentFolders.length) { refuse("Open a workspace folder before changing background-check exclusions."); }
    const selected = target instanceof WorkspaceItem ? target.folder : undefined;
    const folder = selected ?? (currentFolders.length === 1 ? currentFolders[0] :
      (await vscode.window.showQuickPick(currentFolders.map(value => ({ label: value.name, description: value.uri.fsPath, folder: value })),
        { placeHolder: "Choose a folder for background-check exclusions" }))?.folder);
    if (!folder) return;
    if (!folders().some(value => value.uri.toString() === folder.uri.toString())) { refuse("The workspace folder changed. Configure its exclusions again."); }
    const config = vscode.workspace.getConfiguration("dynare", folder.uri), raw: unknown = config.get("workspaceExcludePaths", []);
    const existing = Array.isArray(raw) ? raw.filter((entry): entry is string => typeof entry === "string") : [];
    const action = await vscode.window.showQuickPick([
      { label: "Add exclusion…", action: "add" }, { label: "Remove exclusion…", action: "remove" },
      { label: "Reset folder exclusions", action: "reset" }, { label: "Open workspace settings", action: "settings" },
    ], { placeHolder: `Background-check exclusions for ${folder.name}` });
    if (!action) return;
    if (!folders().some(value => value.uri.toString() === folder.uri.toString())) { refuse("The workspace folder changed. Configure its exclusions again."); }
    if (action.action === "settings") { await vscode.commands.executeCommand("workbench.action.openSettings", "@id:dynare.workspaceExcludePaths"); return; }
    let value: string[] | undefined;
    if (action.action === "add") {
      const pattern = await vscode.window.showInputBox({ prompt: `Exclude background root models relative to ${folder.name}`, placeHolder: "generated/**", validateInput: text => text.trim() ? undefined : "Enter a folder-relative pattern." });
      if (!pattern?.trim()) return; value = [...new Set([...existing, pattern.trim()])];
    } else if (action.action === "remove") {
      const pattern = await vscode.window.showQuickPick(existing, { placeHolder: "Choose an exclusion to remove" });
      if (pattern === undefined) return; value = existing.filter(entry => entry !== pattern);
    }
    if (disposed) return;
    if (!folders().some(currentFolder => currentFolder.uri.toString() === folder.uri.toString())) refuse("The workspace folder changed. Configure its exclusions again.");
    await config.update("workspaceExcludePaths", value, vscode.ConfigurationTarget.WorkspaceFolder);
  };
  const ignoreFolder = async (target: unknown): Promise<void> => {
    // Native row actions marshal the item as plain data, without its class prototype.
    if (!record(target) || !record(target.folder) || typeof target.contextValue !== "string" ||
        !["dygnosis.workspaceFolder", "dygnosis.workspaceDirectory"].includes(target.contextValue)) {
      refuse("Choose a folder in Dynare workspace checks before ignoring it.");
    }
    const filePath = (value: unknown): string | undefined => record(value) && value.scheme === "file" &&
      typeof value.fsPath === "string" && path.isAbsolute(value.fsPath) ? value.fsPath : undefined;
    const ownerPath = filePath(target.folder.uri), directoryPath = filePath(target.resourceUri);
    if (ownerPath === undefined || directoryPath === undefined) refuse("The folder action has no saved folder path. Choose a folder in workspace checks again.");
    const owner = vscode.Uri.file(ownerPath), directoryUri = vscode.Uri.file(directoryPath), currentFolders = folders();
    const folder = currentFolders.find(value => identity(value.uri) === identity(owner));
    if (!folder || containingFolder(directoryUri, currentFolders) !== folder) {
      refuse("The workspace folder changed. Choose its folder in workspace checks again.");
    }
    const relative = path.relative(folder.uri.fsPath, directoryUri.fsPath);
    if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
      refuse("This folder is outside its workspace folder. Choose a folder in workspace checks again.");
    }
    if (relative.split(path.sep).some(part => /[*?\\]/.test(part))) {
      refuse("This folder name contains wildcard or path-separator characters that workspace exclusions cannot match literally. No exclusion was added. Use Configure workspace exclusions to choose a broader path.");
    }
    const pattern = relative ? `${relative.split(path.sep).join("/")}/**` : "**";
    const config = vscode.workspace.getConfiguration("dynare", folder.uri), raw: unknown = config.get("workspaceExcludePaths", []);
    const existing = Array.isArray(raw) ? raw.filter((entry): entry is string => typeof entry === "string") : [];
    if (!disposed && !existing.includes(pattern)) await config.update("workspaceExcludePaths", [...existing, pattern], vscode.ConfigurationTarget.WorkspaceFolder);
  };
  const runAction = (kind: "recheck" | "cancel"): Promise<void> => {
    if (!capability) { render(); refuse("Workspace checks are unavailable. Enable dynare.workspaceDiagnostics and use an engine that supports workspace checks."); }
    if (kind === "recheck" && status) minPass = status.pass_revision + 1;
    beginBarrier(); ++generation; ++response; cancelRequest();
    if (kind === "recheck") { status = undefined; render(); }
    return refresh(kind === "recheck" ? capability.recheck_command : capability.cancel_command);
  };
  const subscriptions = [
    view.onDidCollapseElement(event => { if (event.element.id) collapsed.add(event.element.id); }),
    view.onDidExpandElement(event => { if (event.element.id) collapsed.delete(event.element.id); }),
    service.onDidChange(synchronize), vscode.window.onDidChangeActiveTextEditor(sendActiveRoot),
    service.onDidUpdateModelInfo(info => {
      if (!disposed && info.client_instance === service.currentInstance && instance === info.client_instance && connection === service.client) sendActiveRoot();
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(reconfigure),
    vscode.workspace.onDidChangeTextDocument(event => { if (event.contentChanges.length) documentChanged(event.document); }),
    vscode.workspace.onDidCloseTextDocument(documentChanged),
    vscode.workspace.onDidChangeConfiguration(event => { if (event.affectsConfiguration("dynare")) reconfigure(); }),
    vscode.commands.registerCommand("dygnosis.workspaceStatus", () => vscode.commands.executeCommand("dygnosis.workspace.focus")),
    vscode.commands.registerCommand("dygnosis.recheckWorkspace", () => runCommand(service, () => runAction("recheck"), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.cancelWorkspace", () => runCommand(service, () => runAction("cancel"), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.configureWorkspaceExclusions", (target: unknown) => runCommand(service, () => configureExclusions(target), () => !disposed)),
    vscode.commands.registerCommand("dygnosis.ignoreWorkspaceFolder", (target: unknown) => runCommand(service, () => ignoreFolder(target), () => !disposed)),
  ];
  if (service.onDidReady) subscriptions.push(service.onDidReady(synchronize));
  synchronize(); startFolders();
  return new vscode.Disposable(() => {
    disposed = true; ++generation; ++response; cancelRequest(); notification?.dispose();
    bufferedNotification = undefined;
    if (inputTimer) clearTimeout(inputTimer);
    clearWatchers(watchers); clearWatchers(dependencies);
    dependencyTargets.clear();
    for (const subscription of subscriptions) subscription.dispose();
    tree.dispose(); view.dispose(); item.hide(); item.dispose();
  });
}
