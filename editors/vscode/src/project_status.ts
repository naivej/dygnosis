import * as path from "node:path";
import * as vscode from "vscode";
import { record } from "./protocol";
import { projectDiagnosticsSetting } from "./settings";
import type { ModelSnapshot } from "./client";

const states = ["pending", "checking", "checked", "incomplete", "failed", "excluded"] as const;
type RootState = typeof states[number];
type DiscoveryState = "pending" | "discovering" | "complete" | "cancelled" | "disabled";
export interface ProjectRoot {
  root_uri: string; state: RootState; revision: string | null;
  errors: number; warnings: number; failure: string | null; dependency_candidates: string[];
}
export interface ProjectStatus {
  schema_version: 1; pass_revision: number; enabled: boolean; discovery: DiscoveryState;
  cancelled: boolean; complete: boolean; coverage_complete: boolean;
  counts: Record<RootState, number>; roots: ProjectRoot[];
  discovery_failures: { folder_uri: string; failure: string }[];
  metrics: { discovery_ms: number; analysis_ms: number; completed_jobs: number; reused_jobs: number; elapsed_ms: number | null };
}
export interface ProjectCapability {
  schema_version: 1; status_command: "dynare/projectStatus"; recheck_command: "dynare/recheckProject";
  cancel_command: "dynare/cancelProject"; active_model_notification: "dynare/activeModelChanged";
  status_notification: "dynare/projectStatusChanged";
}
interface ProjectConnection {
  readonly initializeResult?: unknown;
  onNotification(method: string, handler: (value: unknown) => void): vscode.Disposable;
  sendNotification(method: string, params: unknown): Promise<void>;
  isRunning?(): boolean;
}
/** Root selection is a synchronous read of proven context, never owner discovery. */
export interface ProjectClientPort {
  readonly client: ProjectConnection | undefined; readonly currentInstance: number;
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
  return uri.scheme === "file" && process.platform === "win32" ? uri.fsPath.toLowerCase() : uri.toString();
}
function nullableText(value: unknown): value is string | null { return value === null || typeof value === "string"; }
function diagnostics(count: number, noun: "Error" | "Warning"): string { return `${count} ${noun}${count === 1 ? "" : "s"}`; }
/** All required fields are checked; future additive fields are harmless. */
export function parseProjectStatus(value: unknown): ProjectStatus {
  const invalid = (): never => { throw new Error("Unsupported project status. Use the bundled binary or update dynare.serverPath."); };
  if (!record(value) || value.schema_version !== 1 || !integer(value.pass_revision) ||
      typeof value.enabled !== "boolean" || typeof value.cancelled !== "boolean" || typeof value.complete !== "boolean" ||
      typeof value.coverage_complete !== "boolean" || typeof value.discovery !== "string" || !["pending", "discovering", "complete", "cancelled", "disabled"].includes(value.discovery) ||
      !record(value.counts) || !Array.isArray(value.roots) || !Array.isArray(value.discovery_failures) || !record(value.metrics)) return invalid();
  const counts = {} as Record<RootState, number>;
  for (const state of states) { const count = value.counts[state]; if (!integer(count)) return invalid(); counts[state] = count; }
  const rootKeys = new Set<string>();
  const roots = value.roots.map((root: unknown): ProjectRoot => {
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
  const metrics = value.metrics;
  if (!duration(metrics.discovery_ms) || !duration(metrics.analysis_ms) || !integer(metrics.completed_jobs) ||
      !integer(metrics.reused_jobs) || (metrics.elapsed_ms !== null && !duration(metrics.elapsed_ms))) return invalid();
  if ((!value.enabled && (value.discovery !== "disabled" || value.complete || value.coverage_complete)) ||
      (value.complete && (value.cancelled || value.discovery !== "complete" || counts.pending > 0 || counts.checking > 0)) ||
      (value.coverage_complete && (!value.complete || failures.length > 0 || counts.failed > 0 || counts.incomplete > 0))) return invalid();
  return { schema_version: 1, pass_revision: value.pass_revision, enabled: value.enabled, discovery: value.discovery as DiscoveryState,
    cancelled: value.cancelled, complete: value.complete, coverage_complete: value.coverage_complete, counts, roots, discovery_failures: failures,
    metrics: { discovery_ms: metrics.discovery_ms, analysis_ms: metrics.analysis_ms, completed_jobs: metrics.completed_jobs,
      reused_jobs: metrics.reused_jobs, elapsed_ms: metrics.elapsed_ms } };
}
export function parseProjectCapability(initializeResult: unknown): ProjectCapability | undefined {
  if (!record(initializeResult) || !record(initializeResult.capabilities)) return undefined;
  const capabilities = initializeResult.capabilities, experimental = capabilities.experimental;
  if (!record(experimental) || !record(experimental.dygnosis) || !record(experimental.dygnosis.projectDiagnostics)) return undefined;
  const capability = experimental.dygnosis.projectDiagnostics;
  const expected: ProjectCapability = { schema_version: 1, status_command: "dynare/projectStatus", recheck_command: "dynare/recheckProject",
    cancel_command: "dynare/cancelProject", active_model_notification: "dynare/activeModelChanged", status_notification: "dynare/projectStatusChanged" };
  const commands = record(capabilities.executeCommandProvider) ? capabilities.executeCommandProvider.commands : undefined;
  if (Object.entries(expected).some(([key, value]) => capability[key] !== value) || !Array.isArray(commands) ||
      [expected.status_command, expected.recheck_command, expected.cancel_command].some(command => !commands.includes(command))) return undefined;
  return expected;
}
interface StatusContent { text: string; tooltip: string; accessibilityLabel: string }
export function projectStatusContent(status: ProjectStatus): StatusContent {
  const counts = status.counts, selected = status.roots.length - counts.excluded;
  const finished = counts.checked + counts.incomplete + counts.failed;
  const errors = status.roots.filter(root => root.state === "checked").reduce((sum, root) => sum + root.errors, 0);
  let label: string, icon: string;
  if (!status.enabled) { label = "off"; icon = "circle-slash"; }
  else if (status.cancelled) { label = `cancelled · ${counts.checked}/${selected} checked`; icon = "debug-pause"; }
  else if (status.discovery === "pending" || status.discovery === "discovering") { label = "discovering"; icon = "sync~spin"; }
  else if (!status.complete) { label = `${finished}/${selected} finished`; icon = "sync~spin"; }
  else if (!status.coverage_complete) { label = `incomplete · ${counts.checked}/${selected} checked`; icon = "warning"; }
  else if (!selected) { label = "no root models"; icon = "info"; }
  else { label = `${counts.checked}/${selected} checked${errors ? ` · ${diagnostics(errors, "Error")}` : ""}`; icon = errors ? "error" : "check"; }
  const details = [
    "Project diagnostics: unopened saved .mod models",
    ...states.map(state => `${state[0].toUpperCase()}${state.slice(1)}: ${counts[state]}`),
    `Folder discovery failures: ${status.discovery_failures.length}`,
    "Checked models may contain diagnostic Errors. Incomplete and failed models do not have complete coverage.",
    status.cancelled ? "Cancelled until a file edit/change or Recheck. Completed results remain." :
      !status.enabled ? "Enable dynare.projectDiagnostics in Settings to resume." : "Recheck reruns discovery and all selected models.",
    "Click to open Dynare project checks.",
  ].join("\n");
  return { text: `$(${icon}) Dynare project: ${label}`, tooltip: details, accessibilityLabel: `Dynare project: ${label}. Open project checks.` };
}
class ProjectItem extends vscode.TreeItem {
  constructor(id: string, label: string, icon: string, readonly children: ProjectItem[] = [], readonly folder?: vscode.WorkspaceFolder) {
    super(label, children.length ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
    this.id = id; this.iconPath = new vscode.ThemeIcon(icon);
    this.accessibilityInformation = { label };
  }
}
class ProjectTree implements vscode.TreeDataProvider<ProjectItem>, vscode.Disposable {
  private readonly changed = new vscode.EventEmitter<ProjectItem | undefined>();
  readonly onDidChangeTreeData = this.changed.event;
  private rows: ProjectItem[] = [];
  getTreeItem(item: ProjectItem): vscode.TreeItem { return item; }
  getChildren(item?: ProjectItem): ProjectItem[] { return item?.children ?? this.rows; }
  replace(rows: ProjectItem[]): void { this.rows = rows; this.changed.fire(undefined); }
  dispose(): void { this.rows = []; this.changed.dispose(); }
}
function folders(): readonly vscode.WorkspaceFolder[] { return (vscode.workspace.workspaceFolders ?? []).filter(folder => folder.uri.scheme === "file"); }
function containingFolder(uri: vscode.Uri, candidates: readonly vscode.WorkspaceFolder[]): vscode.WorkspaceFolder | undefined {
  const key = identity(uri);
  return candidates.filter(folder => { const base = identity(folder.uri).replace(/[\\/]$/, ""); return key === base || key.startsWith(`${base}${uri.scheme === "file" && process.platform === "win32" ? path.sep : "/"}`); })
    .sort((left, right) => right.uri.path.length - left.uri.path.length)[0];
}
function treeRows(status: ProjectStatus, currentFolders: readonly vscode.WorkspaceFolder[]): ProjectItem[] {
  return currentFolders.map(folder => {
    const roots = status.roots.filter(root => containingFolder(vscode.Uri.parse(root.root_uri), currentFolders) === folder).map(root => {
      const uri = vscode.Uri.parse(root.root_uri), label = vscode.workspace.asRelativePath(uri, false);
      const detail = `${root.state === "checked" ? "Checked" : root.state[0].toUpperCase() + root.state.slice(1)}${root.state === "checked" ? ` · ${diagnostics(root.errors, "Error")}, ${diagnostics(root.warnings, "Warning")}` : ""}`;
      const item = new ProjectItem(root.root_uri, label, root.state === "failed" || root.state === "incomplete" ? "warning" :
        root.state === "checked" ? root.errors ? "error" : "check" : root.state === "checking" ? "sync~spin" : root.state === "excluded" ? "circle-slash" : "clock");
      item.resourceUri = uri; item.description = detail;
      item.tooltip = `${uri.fsPath}\n${detail}${root.failure ? `\n${root.failure}` : ""}`;
      item.accessibilityInformation = { label: `${label}. ${detail}` };
      item.command = { command: "vscode.open", title: "Open model", arguments: [uri] };
      return item;
    });
    const failures = status.discovery_failures.filter(failure => identity(vscode.Uri.parse(failure.folder_uri)) === identity(folder.uri)).map((failure, index) => {
      const item = new ProjectItem(`${folder.uri.toString()}:failure:${index}`, "Discovery failed", "warning");
      item.description = failure.failure; item.tooltip = failure.failure;
      item.accessibilityInformation = { label: `Discovery failed. ${failure.failure}` }; return item;
    });
    if (!roots.length && !failures.length) roots.push(new ProjectItem(`${folder.uri.toString()}:empty`,
      status.discovery === "pending" || status.discovery === "discovering" ? "Discovering root models…" :
        status.discovery === "cancelled" ? "Discovery cancelled" : "No root .mod models", "info"));
    const row = new ProjectItem(folder.uri.toString(), folder.name, "folder", [...failures, ...roots], folder);
    row.contextValue = "dygnosis.projectFolder"; row.resourceUri = folder.uri;
    return row;
  });
}

/** Presentation and watcher adapter only; analysis stays in the server. */
export function registerProjectStatus(service: ProjectClientPort): vscode.Disposable {
  const item = vscode.window.createStatusBarItem("dygnosis.projectCoverage", vscode.StatusBarAlignment.Left, 39);
  item.name = "Dynare project coverage"; item.command = "dygnosis.projectStatus";
  const tree = new ProjectTree(), view = vscode.window.createTreeView("dygnosis.project", { treeDataProvider: tree, showCollapseAll: true });
  let disposed = false, connection: ProjectConnection | undefined, instance = -1;
  let capability: ProjectCapability | undefined, status: ProjectStatus | undefined, generation = 0, response = 0, minPass = 0;
  let barrier = false, activeRoot: string | null | undefined;
  let bufferedNotification: { value: ProjectStatus; client: ProjectConnection; instance: number; generation: number; settings: string } | undefined;
  let startupFailed = false;
  let request: vscode.CancellationTokenSource | undefined, notification: vscode.Disposable | undefined;
  let inputTimer: ReturnType<typeof setTimeout> | undefined;
  const watchers = new Map<string, vscode.Disposable>();
  const dependencies = new Map<string, vscode.Disposable>();
  const dependencyTargets = new Map<string, Set<string>>();
  const enabled = (): boolean => projectDiagnosticsSetting(message => service.log(message));
  const snapshot = (): string => JSON.stringify([enabled(), folders().map(folder => [folder.uri.toString(),
    vscode.workspace.getConfiguration("dynare", folder.uri).get<unknown>("projectExcludePaths", [])])]);
  const clearWatchers = (owned: Map<string, vscode.Disposable>): void => { for (const watcher of owned.values()) watcher.dispose(); owned.clear(); };
  const cancelRequest = (): void => { request?.cancel(); request?.dispose(); request = undefined; };
  const beginBarrier = (): void => { barrier = true; bufferedNotification = undefined; };
  const message = (label: string, icon = "info", detail = label): void => {
    item.text = `$(${icon}) Dynare project: ${label}`; item.tooltip = detail;
    item.accessibilityInformation = { label: `Dynare project: ${label}. Open project checks.` };
    item.show(); view.message = detail; view.description = undefined; tree.replace([]);
  };
  const render = (): void => {
    if (disposed) return;
    if (!enabled()) { message("off", "circle-slash", "Project diagnostics are off. Enable dynare.projectDiagnostics in Settings."); return; }
    if (!folders().length) { message("no folders", "info", "Open a file-backed workspace folder to check unopened .mod models."); return; }
    if (!status) { message(capability ? "updating" : connection || startupFailed ? "unavailable" : "starting", capability || (!connection && !startupFailed) ? "sync~spin" : "warning",
      capability ? "Refreshing project coverage." : connection ? "Project checks require a compatible engine. Use the bundled binary or update dynare.serverPath." :
        startupFailed ? "Dygnosis is unavailable. Restart the language server or open Dygnosis Output for details." : "Starting Dygnosis project checks."); return; }
    const content = projectStatusContent(status); item.text = content.text; item.tooltip = content.tooltip;
    item.accessibilityInformation = { label: content.accessibilityLabel }; item.show();
    view.message = status.cancelled ? "Cancelled until a file edit/change or Recheck." :
      status.discovery_failures.length ? "Coverage is incomplete: folder discovery failed." :
      status.counts.failed || status.counts.incomplete ? "Coverage is incomplete. Open affected models for details." : undefined;
    view.description = `${status.counts.checked} checked`;
    tree.replace(treeRows(status, folders()));
  };
  const current = (client: ProjectConnection, expectedInstance: number, expectedGeneration: number, expectedSnapshot: string): boolean =>
    !disposed && service.client === client && service.currentInstance === expectedInstance && connection === client &&
    generation === expectedGeneration && snapshot() === expectedSnapshot && (client.isRunning?.() ?? true);
  const refresh = async (command?: string): Promise<void> => {
    if (!connection && (!enabled() || !folders().length)) return;
    await service.ensureStarted(); synchronize();
    const client = connection, supported = capability;
    if (disposed || !client || !supported) return;
    cancelRequest(); const cancellation = new vscode.CancellationTokenSource(); request = cancellation;
    const expectedGeneration = generation, expectedInstance = instance, expectedSnapshot = snapshot(), serial = ++response;
    try {
      const value = await service.execute(command ?? supported.status_command, [], cancellation.token);
      if (cancellation.token.isCancellationRequested || serial !== response || !current(client, expectedInstance, expectedGeneration, expectedSnapshot)) return;
      const next = parseProjectStatus(value);
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
        barrier = false; bufferedNotification = undefined; status = undefined; clearWatchers(dependencies); service.log(String(error));
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
      if (!dependencies.has(key)) dependencies.set(key, watch(new vscode.RelativePattern(group.directory, "*"), uri => dependencyTargets.get(key)?.has(identity(uri)) ?? false));
    }
  }
  const updateRootWatchers = (): void => {
    clearWatchers(watchers);
    if (!enabled() || !capability || !connection) return;
    for (const folder of folders()) watchers.set(folder.uri.toString(), watch(new vscode.RelativePattern(folder.uri, "**/*.mod"), () => true));
  };
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
    capability = parseProjectCapability(connection?.initializeResult);
    if (connection && capability) {
      const subscribedClient = connection, subscribedInstance = instance;
      notification = connection.onNotification(capability.status_notification, (value: unknown) => {
        if (disposed || connection !== subscribedClient || service.client !== subscribedClient || instance !== subscribedInstance ||
            service.currentInstance !== subscribedInstance || !(subscribedClient.isRunning?.() ?? true)) return;
        try {
          const next = parseProjectStatus(value);
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
    const selected = target instanceof ProjectItem ? target.folder : undefined;
    const folder = selected ?? (currentFolders.length === 1 ? currentFolders[0] :
      (await vscode.window.showQuickPick(currentFolders.map(value => ({ label: value.name, description: value.uri.fsPath, folder: value })),
        { placeHolder: "Choose a folder for background-check exclusions" }))?.folder);
    if (!folder || !folders().some(value => value.uri.toString() === folder.uri.toString())) return;
    const config = vscode.workspace.getConfiguration("dynare", folder.uri), raw: unknown = config.get("projectExcludePaths", []);
    const existing = Array.isArray(raw) ? raw.filter((entry): entry is string => typeof entry === "string") : [];
    const action = await vscode.window.showQuickPick([
      { label: "Add exclusion…", action: "add" }, { label: "Remove exclusion…", action: "remove" },
      { label: "Reset folder exclusions", action: "reset" }, { label: "Open project settings", action: "settings" },
    ], { placeHolder: `Background-check exclusions for ${folder.name}` });
    if (!action || !folders().some(value => value.uri.toString() === folder.uri.toString())) return;
    if (action.action === "settings") { await vscode.commands.executeCommand("workbench.action.openSettings", "@id:dynare.projectExcludePaths"); return; }
    let value: string[] | undefined;
    if (action.action === "add") {
      const pattern = await vscode.window.showInputBox({ prompt: `Exclude background root models relative to ${folder.name}`, placeHolder: "generated/**", validateInput: text => text.trim() ? undefined : "Enter a folder-relative pattern." });
      if (!pattern?.trim()) return; value = [...new Set([...existing, pattern.trim()])];
    } else if (action.action === "remove") {
      const pattern = await vscode.window.showQuickPick(existing, { placeHolder: "Choose an exclusion to remove" });
      if (pattern === undefined) return; value = existing.filter(entry => entry !== pattern);
    }
    if (!disposed && folders().some(currentFolder => currentFolder.uri.toString() === folder.uri.toString()))
      await config.update("projectExcludePaths", value, vscode.ConfigurationTarget.WorkspaceFolder);
  };
  const runAction = (kind: "recheck" | "cancel"): void => {
    if (!capability) { render(); return; }
    if (kind === "recheck" && status) minPass = status.pass_revision + 1;
    beginBarrier(); ++generation; ++response; cancelRequest();
    if (kind === "recheck") { status = undefined; render(); }
    void refresh(kind === "recheck" ? capability.recheck_command : capability.cancel_command);
  };
  const subscriptions = [
    service.onDidChange(synchronize), vscode.window.onDidChangeActiveTextEditor(sendActiveRoot),
    service.onDidUpdateModelInfo(info => {
      if (!disposed && info.client_instance === service.currentInstance && instance === info.client_instance && connection === service.client) sendActiveRoot();
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(reconfigure),
    vscode.workspace.onDidChangeTextDocument(event => { if (event.contentChanges.length) documentChanged(event.document); }),
    vscode.workspace.onDidCloseTextDocument(documentChanged),
    vscode.workspace.onDidChangeConfiguration(event => { if (event.affectsConfiguration("dynare")) reconfigure(); }),
    vscode.commands.registerCommand("dygnosis.projectStatus", () => vscode.commands.executeCommand("dygnosis.project.focus")),
    vscode.commands.registerCommand("dygnosis.recheckProject", () => runAction("recheck")),
    vscode.commands.registerCommand("dygnosis.cancelProject", () => runAction("cancel")),
    vscode.commands.registerCommand("dygnosis.configureProjectExclusions", (target: unknown) => configureExclusions(target)),
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
