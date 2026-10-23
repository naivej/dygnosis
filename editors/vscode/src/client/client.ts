import { CommandCancelled, refuse, runCommand } from "./refusal";
import { spawn, ChildProcess } from "node:child_process";
import * as vscode from "vscode";
import {
  ClientCapabilities, DocumentSymbolRequest, ExecuteCommandRequest, FeatureState, InitializeParams,
  LanguageClient, MessageSignature, Middleware, RenameRequest, StaticFeature, State,
} from "vscode-languageclient/node";
import { Binary, resolveBinary } from "./binary";
import { ClientLifecycle, ManagedClient } from "./lifecycle";
import { configurationSnapshot, macroTintOnlyChange } from "./settings";
import { AnalysisConfiguration, AnalysisContext, PresetBinding, ModelInfo, parseAnalysisContext, parseModelInfo, record, timingModifiers, tokenRoles } from "./protocol";
import { renameSymbol } from "./rename";
import { sameSourceUri as sameUri } from "./source_uri";
import { ModelActivity, RequestActivity } from "./activity";

interface ClientProcess extends ManagedClient { readonly client: LanguageClient }
export interface ClientDependencies {
  resolve?: typeof resolveBinary;
  create?: (binary: Binary, output: vscode.OutputChannel, middleware: Middleware, settings: unknown, capture: (params: InitializeParams) => void) => ClientProcess;
}

export const documentSelector = [
  { language: "dynare", scheme: "file" },
  { language: "dynare", scheme: "untitled" },
];
export function isAnalysisDocument(document: vscode.TextDocument): boolean {
  return document.languageId === "dynare" && ["file", "untitled"].includes(document.uri.scheme);
}
export function isRootUri(uri: vscode.Uri): boolean {
  return ["file", "untitled"].includes(uri.scheme) && /\.(mod|dyn)$/i.test(uri.path);
}
export function extendCapabilities(capabilities: ClientCapabilities): void {
  const semantic = capabilities.textDocument?.semanticTokens;
  if (semantic) {
    semantic.tokenTypes = [...new Set([...semantic.tokenTypes, ...tokenRoles])];
    semantic.tokenModifiers = [...new Set([...semantic.tokenModifiers, ...timingModifiers])];
  }
  const experimental: Record<string, unknown> = record(capabilities.experimental) ? capabilities.experimental : {};
  experimental.dygnosis = { modelInfo: { schema_version: 1 }, modelInfoChanged: true, documentContext: { schema_version: 1 }, configuration: { schema_version: 1 },
    analysisConfiguration: { schema_version: 1 },
    compareModels: { navigation_schema_version: 1 }, compareModelSnapshots: { schema_version: 1, navigation_schema_version: 2 }, effectivePreview: { navigation_schema_version: 1, dependency_candidates: true },
    workspaceStatusChanged: true };
  capabilities.experimental = experimental;
}
class DygnosisCapabilities implements StaticFeature {
  constructor(private readonly capture: (params: InitializeParams) => void) {}
  fillClientCapabilities(capabilities: ClientCapabilities): void { extendCapabilities(capabilities); }
  fillInitializeParams(params: InitializeParams): void { this.capture(params); }
  initialize(): void { /* No provider; only an explicit initialize advertisement. */ }
  clear(): void { /* No per-client resources. */ }
  getState(): FeatureState { return { kind: "static" }; }
}

class ProcessClient implements ManagedClient {
  readonly client: LanguageClient;
  private process: ChildProcess | undefined;
  private disposed = false;
  private disposing: Promise<void> | undefined;
  constructor(binary: Binary, output: vscode.OutputChannel, middleware: Middleware, initializationOptions: unknown, capture: (params: InitializeParams) => void) {
    const isDisposed = () => this.disposed;
    class ProcessLanguageClient extends LanguageClient {
      override handleFailedRequest<T>(type: MessageSignature, token: vscode.CancellationToken | undefined, error: unknown, defaultValue: T, showNotification = true): T {
        // A retired provider has no current result; active failures keep the SDK policy.
        if (isDisposed()) return defaultValue;
        return super.handleFailedRequest(type, token, error, defaultValue, showNotification);
      }
    }
    this.client = new ProcessLanguageClient("dygnosis", "Dygnosis", () => {
      if (this.disposed) throw new Error("Dygnosis startup was superseded.");
      const child = spawn(binary.path, [], { windowsHide: true, shell: false, stdio: "pipe" });
      this.process = child;
      child.stderr?.on("data", (data: Buffer) => output.append(data.toString()));
      return Promise.resolve(child);
    }, {
      documentSelector, outputChannel: output, middleware,
      initializationOptions, synchronize: {},
      connectionOptions: { maxRestartCount: 0 },
    });
    this.client.registerFeature(new DygnosisCapabilities(capture));
  }
  start(): Promise<void> { return this.client.start(); }
  dispose(): Promise<void> {
    if (this.disposing) return this.disposing;
    this.disposed = true;
    const running = this.client.state === State.Running;
    // Stop providers and send shutdown while the protocol stream is still open.
    this.disposing = this.client.dispose(1500).catch(() => undefined).finally(() => {
      const child = this.process;
      if (child && child.exitCode === null && child.signalCode === null && !child.killed) child.kill();
    });
    // Initialize cannot be stopped through the protocol. Reject its pending start.
    if (!running) this.process?.kill();
    return this.disposing;
  }
}
interface CachedRequest {
  epoch: number; version: number | null; cancellation: vscode.CancellationTokenSource;
  result: Promise<ModelSnapshot | undefined>;
}
export interface ModelSnapshot extends ModelInfo { client_instance: number }
export interface NavigationDecision { isCurrent(): boolean }
export type NavigationGuard = (loadedDocument: vscode.TextDocument) => boolean | NavigationDecision | Promise<boolean | NavigationDecision>;
export interface InputInvalidation { root?: string; reason: "input" | "file" | "lifecycle"; uri?: string; inputRevision?: string; modelRevision?: string }

/** Shared service for status, view, tint, lenses, diagnostics and previews. */
export class DygnosisClient implements vscode.Disposable {
  readonly output = vscode.window.createOutputChannel("Dygnosis");
  readonly middleware: Middleware = {};
  private readonly activity = new RequestActivity();
  readonly onDidChangeActivity = this.activity.onDidChange;
  private readonly lifecycle = new ClientLifecycle<ClientProcess>();
  private readonly changed = new vscode.EventEmitter<void>();
  readonly onDidChange = this.changed.event;
  private readonly modelInfoUpdated = new vscode.EventEmitter<ModelSnapshot>();
  readonly onDidUpdateModelInfo = this.modelInfoUpdated.event;
  private readonly analysisChanged = new vscode.EventEmitter<{root: string; context: AnalysisContext; client_instance: number}>();
  readonly onDidChangeAnalysis = this.analysisChanged.event;
  private readonly invalidated = new vscode.EventEmitter<InputInvalidation>();
  readonly onDidInvalidate = this.invalidated.event;
  private readonly disposables: vscode.Disposable[] = [this.output, this.activity, this.changed, this.invalidated, this.modelInfoUpdated, this.analysisChanged];
  private analysisSupported = false;
  private readonly analysisAttempts = new Map<string, number>();
  private readonly acceptedAnalysis = new Map<string, number>();
  private readonly analysisInstances = new WeakMap<AnalysisContext, number>();
  private readonly cache = new Map<string, CachedRequest>();
  private readonly freshTails = new Map<string, Promise<void>>();
  private readonly cancelFreshRequests = new Set<() => void>();
  private readonly infos = new Map<string, ModelInfo>();
  private readonly selectedOwners = new Map<string, string>();
  private readonly explicitOwnerDocuments = new Set<string>();
  private readonly ownedDocuments = new Set<string>();
  private readonly standaloneDocuments = new Set<string>();
  // A newer attempt stops stale proofs, but only an accepted choice can replace
  // a delayed server acknowledgment. File lifecycle changes invalidate both.
  private readonly contextGenerations = new Map<string, number>();
  private readonly acceptedContextGenerations = new Map<string, number>();
  private readonly contextLifecycles = new Map<string, number>();
  private documentContextSupported = false;
  private instanceSubscriptions: vscode.Disposable[] = [];
  private instance = 0;
  private readonly dependencyWatchers = new Map<string, vscode.Disposable>();
  private readonly rootCandidates = new Map<string, Set<string>>();
  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  private symbolTimer: ReturnType<typeof setTimeout> | undefined;
  private shutdownPromise: Promise<void> | undefined;
  private epoch = 0;
  private startGeneration = 0;
  private settingsGeneration = 0;
  private readonly includeLinks = new Map<string, { source: vscode.Uri; target: vscode.Uri; version: number; instance: number }>();
  private linkSequence = 0;
  private starting: Promise<void> | undefined;
  private closed = false;
  private reportedFailure: string | undefined;
  private symbolRegistration = false;
  private modelInfoSupported = false;
  lastInitializeParams: InitializeParams | undefined;
  constructor(private readonly context: vscode.ExtensionContext, private readonly dependencies: ClientDependencies = {}) {
    // VS Code's RenameProvider converter drops document versions. Own native
    // application through the public, version-aware workspace.applyEdit API.
    this.middleware.handleRegisterCapability = async (params, next) => {
      const cancellation = new vscode.CancellationTokenSource();
      try { await next({ ...params, registrations: params.registrations.filter(registration => registration.method !== RenameRequest.method) }, cancellation.token); }
      finally { cancellation.dispose(); }
    };
    this.disposables.push(vscode.commands.registerCommand("dygnosis.renameSymbol", () => renameSymbol(this)));
    this.middleware.provideDocumentLinks = async (document, token, next) => {
      const instance = this.instance, client = this.client, version = document.version;
      const links = await next(document, token);
      if (!links || token.isCancellationRequested || client !== this.client || document.version !== version || !this.modelInfoSupported) return links;
      // VS Code may retain an earlier provider response. Keep its command key
      // while the same source version, process and resolved target stay valid.
      // Replace obsolete targets rather than accumulating every provider call.
      for (const [key, link] of this.includeLinks) if (sameUri(link.source, document.uri) &&
        (link.version !== version || link.instance !== instance || !links.some(current => current.target && sameUri(current.target, link.target)))) this.includeLinks.delete(key);
      return links.map(link => {
        const data: unknown = (link as vscode.DocumentLink & {data?: unknown}).data;
        const sourceInclude = record(data) && data.dygnosis_source_include === true;
        if (!link.target || link.target.scheme !== "file" || !sourceInclude && !/\.(mod|dyn|inc)$/i.test(link.target.path)) return link;
        const target = link.target;
        const existing = [...this.includeLinks].find(([, current]) => sameUri(current.source, document.uri) &&
          current.version === version && current.instance === instance && sameUri(current.target, target));
        const key = existing?.[0] ?? String(++this.linkSequence);
        this.includeLinks.set(key, { source: document.uri, target, version, instance });
        const wrapped = new vscode.DocumentLink(link.range, vscode.Uri.parse(`command:dygnosis.openIncludeLink?${encodeURIComponent(JSON.stringify([key]))}`));
        wrapped.tooltip = link.tooltip;
        return wrapped;
      });
    };
    this.disposables.push(vscode.commands.registerCommand("dygnosis.openIncludeLink", (key: unknown) => runCommand(this, () => this.openIncludeLink(key), () => !this.closed)));
    const watcher = vscode.workspace.createFileSystemWatcher("**/*.{mod,dyn,inc}");
    this.disposables.push(this.watch(watcher),
      vscode.workspace.onDidOpenTextDocument(document => {
        if (isAnalysisDocument(document)) void this.ensureStarted().catch(error => this.log(String(error)));
        else if (document.languageId === "dynare" && !document.uri.scheme.startsWith("dygnosis-"))
          this.output.appendLine(`Grammar only for ${document.uri.scheme}: documents; native analysis requires file or untitled.`);
      }),
      vscode.workspace.onDidChangeTextDocument(event => {
        if (event.contentChanges.length && isAnalysisDocument(event.document)) {
          this.pruneSourceLinks(event.document.uri, event.document.version);
          this.invalidate(undefined, event.document.uri);
        }
      }),
      vscode.workspace.onDidCloseTextDocument(document => {
        if (isAnalysisDocument(document)) {
          const key = document.uri.toString();
          this.clearDocumentContext(key); this.pruneSourceLinks(document.uri); this.invalidate(undefined, document.uri);
        }
      }),
      vscode.workspace.onDidChangeWorkspaceFolders(() => { this.clearDocumentContexts(); void this.sendSettings().catch(error => this.log(String(error))); }),
      vscode.workspace.onDidChangeConfiguration(event => {
        if (!event.affectsConfiguration("dynare") || macroTintOnlyChange(event)) return;
        if (event.affectsConfiguration("dynare.serverPath")) void this.restart().catch(error => this.log(String(error)));
        else void this.sendSettings().catch(error => this.log(String(error)));
      }),
    );
    const tabChanges = vscode.window.tabGroups.onDidChangeTabs?.(event => this.closeEditorTabs(event.closed));
    if (tabChanges) this.disposables.push(tabChanges);
  }
  get client(): LanguageClient | undefined { return this.lifecycle.client?.client; }
  get currentInstance(): number { return this.instance; }
  get supportsModelInfo(): boolean { return this.modelInfoSupported; }
  get supportsAnalysisConfiguration(): boolean { return this.analysisSupported; }
  modelActivity(document: vscode.Uri, root?: vscode.Uri): ModelActivity | undefined { return this.activity.forDocument(document, root); }
  private pruneSourceLinks(source: vscode.Uri, version?: number): void {
    for (const [key, link] of this.includeLinks) if (sameUri(link.source, source) && link.version !== version) this.includeLinks.delete(key);
  }
  private closeEditorTabs(closed: readonly vscode.Tab[]): void {
    const remaining = vscode.window.tabGroups.all.flatMap(group => group.tabs.flatMap(tab => textTabUris(tab.input)));
    const documents = new Map<string, vscode.TextDocument>();
    for (const uri of closed.flatMap(tab => textTabUris(tab.input))) {
      if (remaining.some(open => sameUri(open, uri))) continue;
      const document = vscode.workspace.textDocuments.find(candidate => isAnalysisDocument(candidate) && sameUri(candidate.uri, uri));
      if (document) documents.set(document.uri.toString(), document);
    }
    for (const [key, document] of documents) {
      const hasChoice = this.selectedOwners.has(key) || this.standaloneDocuments.has(key) || this.contextGenerations.has(key);
      if (!hasChoice) continue;
      this.clearDocumentContext(key);
      this.changed.fire(); this.scheduleSymbolRefresh();
      // VS Code can retain its TextDocument after the final editor tab closes.
      // Clear the server choice now; a later didClose also clears it normally.
      void this.resetRetainedDocumentContext(document, this.contextLifecycles.get(key)!);
    }
  }
  private async resetRetainedDocumentContext(document: vscode.TextDocument, lifecycle: number): Promise<void> {
    const client = this.client, key = document.uri.toString();
    if (!client || !this.documentContextSupported || this.closed || document.isClosed || this.contextLifecycles.get(key) !== lifecycle) return;
    try {
      const result: unknown = await client.sendRequest(ExecuteCommandRequest.type, { command: "dynare/setDocumentContext", arguments: [{ document_uri: key, document_version: document.version, root_uri: null }] });
      if (record(result) && typeof result.error === "string") throw new Error(result.error);
      if (record(result) && result.success === false && typeof result.message === "string") throw new Error(result.message);
    } catch (error) {
      this.log(`The editor model choice was cleared, but diagnostics could not return to automatic context: ${String(error)}`);
    }
  }
  log = (message: string): void => this.output.appendLine(message);
  private watch(watcher: vscode.FileSystemWatcher, target?: vscode.Uri): vscode.Disposable {
    const listeners: vscode.Disposable[] = [watcher];
    for (const event of [watcher.onDidCreate, watcher.onDidChange, watcher.onDidDelete]) {
      listeners.push(event(uri => {
        if (target && !sameUri(uri, target)) return;
        this.invalidate(undefined, uri);
        void this.client?.sendNotification("workspace/didChangeWatchedFiles", { changes: [{ uri: uri.toString(), type: event === watcher.onDidCreate ? 1 : event === watcher.onDidDelete ? 3 : 2 }] });
      }));
    }
    return vscode.Disposable.from(...listeners);
  }
  invalidate(root?: string, file?: vscode.Uri, inputRevision?: string, modelRevision?: string): void {
    this.invalidateInputs(file ? { root, reason: "file", uri: file.toString() } : { root, reason: "input", ...(inputRevision ? { inputRevision } : {}), ...(modelRevision ? { modelRevision } : {}) });
  }
  private invalidateLifecycle(): void {
    this.activity.reset();
    ++this.instance;
    this.clearDocumentContexts();
    this.documentContextSupported = false;
    this.analysisSupported = false;
    this.analysisAttempts.clear(); this.acceptedAnalysis.clear();
    this.invalidateInputs({ reason: "lifecycle" });
  }
  private invalidateInputs(event: InputInvalidation): void {
    const root = event.root;
    ++this.epoch;
    // Every in-flight snapshot uses the global epoch. Release its fresh
    // callers immediately; a server ignoring cancellation must not hold a queue.
    for (const cancel of this.cancelFreshRequests) cancel();
    for (const [key, request] of this.cache) {
      if (!root || key.startsWith(`${root}\n`)) { request.cancellation.cancel(); request.cancellation.dispose(); this.cache.delete(key); }
    }
    if (root) this.infos.delete(root); else this.infos.clear();
    this.invalidated.fire(event);
    this.changed.fire();
    this.scheduleRootRefresh();
    this.scheduleSymbolRefresh();
  }
  private scheduleSymbolRefresh(): void {
    if (this.closed || !this.client) return;
    if (this.symbolTimer) clearTimeout(this.symbolTimer);
    this.symbolTimer = setTimeout(() => { this.symbolTimer = undefined; this.refreshSymbols(); }, 75);
  }
  private scheduleRootRefresh(): void {
    if (this.closed || !this.modelInfoSupported) return;
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = undefined;
      const roots = new Set(vscode.workspace.textDocuments.filter(doc => isRootUri(doc.uri) && !this.ownedDocuments.has(doc.uri.toString())).map(doc => doc.uri.toString()));
      for (const root of this.selectedOwners.values()) roots.add(root);
      for (const root of this.rootCandidates.keys()) if (!roots.has(root)) this.rootCandidates.delete(root);
      this.pruneDependencyWatchers();
      void Promise.all([...roots].map(root => this.modelInfo(vscode.Uri.parse(root)))).catch(error => this.log(String(error)));
    }, 75);
  }
  async ensureStarted(): Promise<void> {
    if (this.closed) return;
    if (this.starting) return this.starting;
    if (this.client) return;
    const starting = this.start().finally(() => {
      if (this.starting === starting) this.starting = undefined;
    });
    this.starting = starting;
    return starting;
  }
  private async start(): Promise<void> {
    const generation = ++this.startGeneration;
    this.modelInfoSupported = false;
    this.symbolRegistration = false;
    this.includeLinks.clear();
    for (const subscription of this.instanceSubscriptions) subscription.dispose();
    this.instanceSubscriptions = [];
    try {
      await this.lifecycle.stop();
      if (this.closed || generation !== this.startGeneration) return;
      const binary = await (this.dependencies.resolve ?? resolveBinary)(this.context, this.log);
      if (this.closed || generation !== this.startGeneration) return;
      this.log(`LSP executable: ${binary.path}`);
      const create = this.dependencies.create ?? ((...args: ConstructorParameters<typeof ProcessClient>) => new ProcessClient(...args));
      await this.lifecycle.replace(() => create(binary, this.output, this.activity.middleware(this.middleware, document => this.selectedOwners.get(document) ??
        (isRootUri(vscode.Uri.parse(document)) && !this.ownedDocuments.has(document) ? document : undefined)), configurationSnapshot(this.log), params => { this.lastInitializeParams = params; }), async managed => {
        this.reportedFailure = undefined;
        const client = managed.client;
        client.getFeature(RenameRequest.method).clear();
        ++this.instance;
        await this.reconcileStartup(client);
        if (this.closed || generation !== this.startGeneration || client !== this.client) return;
        this.log(`Engine: ${client.initializeResult?.serverInfo?.name ?? "unknown"} ${client.initializeResult?.serverInfo?.version ?? "unknown version"}`);
        const experimental: unknown = client.initializeResult?.capabilities.experimental;
        this.modelInfoSupported = record(experimental) && record(experimental.dygnosis) &&
          record(experimental.dygnosis.modelInfo) && experimental.dygnosis.modelInfo.schema_version === 1;
        this.documentContextSupported = record(experimental) && record(experimental.dygnosis) &&
          record(experimental.dygnosis.documentContext) && experimental.dygnosis.documentContext.schema_version === 1 &&
          experimental.dygnosis.documentContext.command === "dynare/setDocumentContext";
        this.analysisSupported = record(experimental) && record(experimental.dygnosis) &&
          record(experimental.dygnosis.analysisConfiguration) && experimental.dygnosis.analysisConfiguration.schema_version === 1;
        this.instanceSubscriptions.push(client.onNotification("dynare/modelInfoChanged", (value: unknown) => {
          if (client === this.client && record(value) && value.schema_version === 1 && typeof value.root_uri === "string")
            this.invalidate(value.root_uri, undefined, typeof value.input_revision === "string" ? value.input_revision : undefined,
              typeof value.revision === "string" ? value.revision : undefined);
        }), client.onDidChangeState(event => {
          if (client === this.client && event.newState === State.Stopped) { this.modelInfoSupported = false; this.invalidateLifecycle(); }
        }));
        this.refreshSymbols();
        this.invalidate();
        if (!this.modelInfoSupported) this.log("This engine does not support model information. Update dynare.serverPath or use the bundled binary.");
      });
    } catch (error) {
      if (!this.closed && generation === this.startGeneration) {
        this.activity.reset();
        throw new Error(`Dygnosis could not start. Reinstall the extension or check dynare.serverPath. ${String(error)}`, { cause: error });
      }
    }
  }
  async restart(): Promise<void> {
    if (this.closed) return;
    this.modelInfoSupported = false;
    this.symbolRegistration = false;
    this.invalidateLifecycle();
    // start() replaces and disposes the prior client, including a pending start.
    const replacement = this.start();
    this.starting = replacement;
    try { await replacement; }
    finally { if (this.starting === replacement) this.starting = undefined; }
  }
  async sendSettings(): Promise<void> {
    ++this.settingsGeneration;
    this.invalidate();
    await this.client?.sendNotification("workspace/didChangeConfiguration", { settings: configurationSnapshot(this.log) });
    this.refreshSymbols();
  }
  private async reconcileStartup(client: LanguageClient): Promise<void> {
    let folders = this.lastInitializeParams?.workspaceFolders ?? [];
    let generation: number;
    do {
      generation = this.settingsGeneration;
      const current = (vscode.workspace.workspaceFolders ?? []).map(folder => ({ uri: folder.uri.toString(), name: folder.name }));
      const added = current.filter(folder => !folders.some(previous => previous.uri === folder.uri));
      const removed = folders.filter(folder => !current.some(present => present.uri === folder.uri));
      if (added.length || removed.length) await client.sendNotification("workspace/didChangeWorkspaceFolders", { event: { added, removed } });
      folders = current;
      await client.sendNotification("workspace/didChangeConfiguration", { settings: configurationSnapshot(this.log) });
    } while (!this.closed && client === this.client && generation !== this.settingsGeneration);
  }
  private refreshSymbols(): void {
    const client = this.client;
    if (!client?.initializeResult?.capabilities.documentSymbolProvider) return;
    const feature = client.getFeature(DocumentSymbolRequest.method);
    if (!this.symbolRegistration) { feature.clear(); this.symbolRegistration = true; }
    else feature.unregister("dygnosis-outline");
    feature.register({ id: "dygnosis-outline", registerOptions: { documentSelector } });
  }
  async execute(command: string, args: unknown[], token?: vscode.CancellationToken): Promise<unknown> {
    await this.ensureStarted();
    const client = this.client;
    if (!client) throw new Error("Dygnosis is unavailable. Open Dygnosis Output or restart the language server.");
    if (!client.initializeResult?.capabilities.executeCommandProvider?.commands.includes(command)) throw new Error(`The selected engine does not support ${command}. Use the bundled binary or update dynare.serverPath.`);
    const result: unknown = await client.sendRequest(ExecuteCommandRequest.type, { command, arguments: args }, token);
    if (record(result) && typeof result.error === "string") throw new Error(result.error);
    if (record(result) && result.success === false && typeof result.message === "string") throw new Error(result.message);
    return result;
  }
  async modelInfo(root: vscode.Uri, document = root, fresh = false, token?: vscode.CancellationToken): Promise<ModelSnapshot | undefined> {
    if (token?.isCancellationRequested) return undefined;
    await this.ensureStarted();
    if (this.closed || token?.isCancellationRequested) return undefined;
    if (!this.modelInfoSupported) refuse("The selected engine does not support model information. Use the bundled binary or update dynare.serverPath.");
    return fresh ? this.freshModelInfo(root, document, token) : this.requestModelInfo(root, document);
  }
  async analysisContext(root: vscode.Uri, document = root, token?: vscode.CancellationToken, preparePreset = false): Promise<AnalysisContext | undefined> {
    await this.ensureStarted();
    if (this.closed || token?.isCancellationRequested) return undefined;
    if (!this.analysisSupported) refuse("This engine does not support analysis configurations. Use the bundled engine or update dynare.serverPath.");
    const instance = this.instance, epoch = this.epoch;
    const version = vscode.workspace.textDocuments.find(doc => sameUri(doc.uri, document))?.version ?? null;
    const raw = await this.execute("dynare/analysisContext", [{root_uri:root.toString(), document_uri:document.toString(), document_version:version, prepare_preset:preparePreset}], token);
    if (this.closed || token?.isCancellationRequested || instance !== this.instance || epoch !== this.epoch ||
        version !== (vscode.workspace.textDocuments.find(doc => sameUri(doc.uri, document))?.version ?? null)) return undefined;
    const context = parseAnalysisContext(raw, root.toString());
    if (context.document_uri !== document.toString() || context.document_version !== version) return undefined;
    this.analysisInstances.set(context, instance);
    return context;
  }
  async setAnalysisConfiguration(root: vscode.Uri, choices: AnalysisConfiguration, proof: AnalysisContext, includePaths: string[] | null,
    presetBindings?: PresetBinding[], guard: () => boolean = () => true, token?: vscode.CancellationToken): Promise<AnalysisContext> {
    await this.ensureStarted();
    const key = root.toString(), instance = this.instance;
    if (this.closed || !this.analysisSupported || this.analysisInstances.get(proof) !== instance || proof.root_uri !== key || !guard() || token?.isCancellationRequested)
      refuse("The analysis selector is no longer current. Open it again before changing the configuration.");
    const attempt = (this.analysisAttempts.get(key) ?? 0) + 1;
    this.analysisAttempts.set(key, attempt);
    let raw: unknown;
    try {
      raw = await this.execute("dynare/setAnalysisConfiguration", [{root_uri:key, document_uri:proof.document_uri, document_version:proof.document_version,
        expected_revision:proof.revision, expected_selection_generation:proof.analysis.selection_generation, analysis:choices,
        include_paths:includePaths, ...(presetBindings ? {preset_bindings:presetBindings} : {})}], token);
    } finally {
      // A cancelled or interrupted response can still follow an accepted server
      // mutation. Clear dependent displays and reacquire authoritative state.
      if (!this.closed && instance === this.instance) this.invalidate(key);
    }
    if (this.closed || instance !== this.instance) refuse("The engine restarted while changing the analysis configuration. Open the selector again.");
    const context = parseAnalysisContext(raw, key);
    const generation = context.analysis.selection_generation;
    if ((this.acceptedAnalysis.get(key) ?? -1) > generation) refuse("A newer accepted analysis configuration replaced this result. Run the action again.");
    this.acceptedAnalysis.set(key, generation);
    this.analysisInstances.set(context, instance);
    this.analysisChanged.fire({root:key, context, client_instance:instance});
    if (context.partial || !context.revision) refuse(context.message || "The analysis configuration was applied, but its current source could not be guarded. Refresh the selector before another action.");
    const currentVersion = vscode.workspace.textDocuments.find(doc => sameUri(doc.uri, vscode.Uri.parse(proof.document_uri)))?.version ?? null;
    if (this.analysisAttempts.get(key) !== attempt || token?.isCancellationRequested || !guard() || currentVersion !== proof.document_version)
      refuse("The analysis configuration was applied, but its pending action stopped because a newer choice or source change arrived. Run the action again.");
    return context;
  }
  async validateComparisonInputs(inputs: unknown, token?: vscode.CancellationToken): Promise<boolean> {
    await this.ensureStarted();
    if (this.closed || token?.isCancellationRequested) return false;
    const instance = this.instance;
    const sides = record(inputs) ? [inputs.before, inputs.after] : inputs;
    const raw = await this.execute("dynare/validateComparisonInputs", [{inputs:sides}], token);
    if (this.closed || token?.isCancellationRequested || instance !== this.instance) return false;
    if (!record(raw) || raw.schema_version !== 1 || typeof raw.current !== "boolean")
      refuse("The engine could not validate the captured comparison. Refresh Model Diff or use the bundled engine.");
    return raw.current;
  }
  /** Fresh navigators sharing a root/document cannot cancel one another. */
  private freshModelInfo(root: vscode.Uri, document: vscode.Uri, token?: vscode.CancellationToken): Promise<ModelSnapshot | undefined> {
    const documentKey = document.toString(), key = `${root.toString()}\n${documentKey}`;
    const epoch = this.epoch, instance = this.client;
    const version = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === documentKey)?.version ?? null;
    let cancelled = false, active: CachedRequest | undefined, abort: () => void = () => {};
    const aborted = new Promise<undefined>(resolve => { abort = () => { resolve(undefined); }; });
    const cancel = (): void => {
      cancelled = true; abort();
      if (active) {
        active.cancellation.cancel(); active.cancellation.dispose();
        if (this.cache.get(key) === active) this.cache.delete(key);
      }
    };
    this.cancelFreshRequests.add(cancel);
    const callerSubscription = token?.onCancellationRequested(cancel);
    if (token?.isCancellationRequested) cancel();
    const previous = this.freshTails.get(key) ?? Promise.resolve();
    const task = previous.then(() => {
      const currentVersion = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === documentKey)?.version ?? null;
      if (cancelled || this.closed || epoch !== this.epoch || instance !== this.client || version !== currentVersion) return undefined;
      const result = this.requestModelInfo(root, document, true);
      active = this.cache.get(key);
      return Promise.race([result, aborted]);
    });
    const result = Promise.race([task, aborted]);
    // A cancelled queued caller returns immediately, but its slot must still
    // wait for its predecessor so later live callers cannot bypass that proof.
    const tail = task.then(() => {}, () => {});
    this.freshTails.set(key, tail);
    const release = (): void => { this.cancelFreshRequests.delete(cancel); callerSubscription?.dispose(); };
    void result.then(release, release);
    void tail.then(() => {
      if (this.freshTails.get(key) === tail) this.freshTails.delete(key);
    });
    return result;
  }
  private requestModelInfo(root: vscode.Uri, document: vscode.Uri, fresh = false): Promise<ModelSnapshot | undefined> {
    if (!this.modelInfoSupported || this.closed) return Promise.resolve(undefined);
    const rootKey = root.toString(), documentKey = document.toString(), key = `${rootKey}\n${documentKey}`;
    const version = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === documentKey)?.version ?? null;
    const existing = this.cache.get(key);
    if (!fresh && existing?.epoch === this.epoch && existing.version === version) return existing.result;
    existing?.cancellation.cancel();
    existing?.cancellation.dispose();
    const cancellation = new vscode.CancellationTokenSource();
    const epoch = this.epoch, instance = this.client;
    const result = this.execute("dynare/modelInfo", [{ root_uri: rootKey, document_uri: documentKey }], cancellation.token).then(value => {
      if (cancellation.token.isCancellationRequested || epoch !== this.epoch || instance !== this.client) return undefined;
      const info = parseModelInfo(value, rootKey, documentKey);
      const currentVersion = vscode.workspace.textDocuments.find(doc => doc.uri.toString() === documentKey)?.version ?? null;
      if (info.document_version !== currentVersion) return undefined;
      const ownersBefore = this.knownOwners(document).map(owner => owner.toString()).sort().join("\n");
      this.infos.set(rootKey, info);
      this.watchDependencies(info);
      const snapshot = { ...info, client_instance: this.instance };
      this.modelInfoUpdated.fire(snapshot);
      if (ownersBefore !== this.knownOwners(document).map(owner => owner.toString()).sort().join("\n")) this.changed.fire();
      return snapshot;
    }).catch((error: unknown) => {
      if (cancellation.token.isCancellationRequested || this.closed) return undefined;
      throw error;
    });
    this.cache.set(key, { epoch, version, cancellation, result });
    return result;
  }
  async revalidate(root: vscode.Uri, expectedRevision: string, expectedInstance: number, document = root, token?: vscode.CancellationToken): Promise<ModelSnapshot | undefined> {
    if (expectedInstance !== this.instance || token?.isCancellationRequested) return undefined;
    const info = await this.modelInfo(root, document, true, token);
    return info?.revision === expectedRevision && info.client_instance === expectedInstance ? info : undefined;
  }
  private watchDependencies(info: ModelInfo): void {
    const candidates = info.dependency_candidates ?? info.related_files.flatMap(file => file.path ? [vscode.Uri.file(file.path).toString()] : []);
    this.rootCandidates.set(info.root_uri, new Set(candidates));
    this.pruneDependencyWatchers();
    for (const candidate of candidates) {
      if (this.dependencyWatchers.has(candidate)) continue;
      const uri = vscode.Uri.parse(candidate);
      if (uri.scheme !== "file") continue;
      const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(vscode.Uri.file(requireDirectory(uri.fsPath)), "*"));
      this.dependencyWatchers.set(candidate, this.watch(watcher, uri));
    }
  }
  private pruneDependencyWatchers(): void {
    const needed = new Set([...this.rootCandidates.values()].flatMap(candidates => [...candidates]));
    for (const [candidate, watcher] of this.dependencyWatchers) if (!needed.has(candidate)) {
      watcher.dispose(); this.dependencyWatchers.delete(candidate);
    }
  }
  knownOwners(document: vscode.Uri): vscode.Uri[] {
    const roots = [...this.infos.values()].flatMap(info => [
      ...(!sameUri(vscode.Uri.parse(info.root_uri), document) && this.executesDocument(info, document) ? [info.root_uri] : []),
      ...(sameUri(vscode.Uri.parse(info.document_uri), document) ? info.owner_roots.filter(root => !sameUri(vscode.Uri.parse(root), document)) : []),
    ]);
    return [...new Set(roots)].map(root => vscode.Uri.parse(root));
  }
  /** Written include paths with a current known owner, for resource menus. */
  knownIncludedFiles(): string[] {
    const files = [...this.infos.values()].flatMap(info => info.related_files
      .filter(file => file.kind === "include" && file.resolved && file.path)
      .map(file => vscode.Uri.file(file.path!)));
    const open = vscode.workspace.textDocuments.filter(document => files.some(file => sameUri(file, document.uri)));
    return [...new Set([...files.map(file => file.path), ...open.map(document => document.uri.path)])];
  }
  /** Read proven chosen context without requests, discovery, or a picker. */
  chosenRootForDocument(document: vscode.TextDocument): vscode.Uri | undefined {
    if (this.closed || document.isClosed || !isAnalysisDocument(document)) return undefined;
    const key = document.uri.toString(), selected = this.selectedOwners.get(key);
    if (this.standaloneDocuments.has(key)) return document.uri;
    if (selected) {
      if (!this.explicitOwnerDocuments.has(key)) return undefined;
      const info = this.infos.get(selected);
      return info && this.ownsDocument(info, document.uri) ? vscode.Uri.parse(selected) : undefined;
    }
    return isRootUri(document.uri) && !this.ownedDocuments.has(key) && this.knownOwners(document.uri).length === 0 ? document.uri : undefined;
  }
  async ownerChoices(document: vscode.TextDocument, token?: vscode.CancellationToken): Promise<vscode.Uri[]> {
    if (token?.isCancellationRequested) return [];
    // A document cannot prove itself to be its parent. Its ordinary model-info
    // request owns cancellation; do not block actions on a second self-discovery
    // request before that action can create its cancellable result.
    const roots = new Set(vscode.workspace.textDocuments.filter(doc => isRootUri(doc.uri) && !sameUri(doc.uri, document.uri) && !this.ownedDocuments.has(doc.uri.toString())).map(doc => doc.uri.toString()));
    for (const root of [...this.selectedOwners.values(), ...this.knownOwners(document.uri).map(owner => owner.toString())]) if (!sameUri(vscode.Uri.parse(root), document.uri)) roots.add(root);
    await Promise.all([...roots].map(root => {
      const uri = vscode.Uri.parse(root);
      return this.modelInfo(uri, uri, false, token);
    }));
    if (token?.isCancellationRequested) return [];
    return this.knownOwners(document.uri);
  }
  async rootForDocument(document: vscode.TextDocument, prompt = false, token?: vscode.CancellationToken): Promise<vscode.Uri | undefined> {
    if (!isAnalysisDocument(document) || token?.isCancellationRequested) return undefined;
    const key = document.uri.toString(), selected = this.selectedOwners.get(key);
    if (this.standaloneDocuments.has(key)) return document.uri;
    if (selected) {
      const root = vscode.Uri.parse(selected);
      const info = await this.modelInfo(root, root, false, token);
      if (token?.isCancellationRequested) return undefined;
      if (this.selectedOwners.get(key) !== selected) return undefined;
      if (!info) return undefined;
      if (this.ownsDocument(info, document.uri)) {
        const retainedBrowsing = info.browsing_documents?.some(uri => sameUri(vscode.Uri.parse(uri), document.uri)) === true;
        if (this.explicitOwnerDocuments.has(key) || retainedBrowsing) return root;
        // An automatic sole owner must yield when a second executing root
        // appears. Only an accepted browsing choice survives exclusion.
      } else {
        this.selectedOwners.delete(key);
        this.explicitOwnerDocuments.delete(key);
        this.ownedDocuments.delete(key);
      }
    }
    // Discover owners only from available models, never analyze an include as a root.
    const selection = this.selectedOwners.get(key);
    const owners = await this.ownerChoices(document, token);
    if (token?.isCancellationRequested) return undefined;
    if (this.selectedOwners.get(key) !== selection) return undefined;
    if (owners.length === 1) { this.rememberOwner(document.uri, owners[0], false); return owners[0]; }
    if (owners.length === 0) {
      this.selectedOwners.delete(key); this.ownedDocuments.delete(key);
      return isRootUri(document.uri) ? document.uri : undefined;
    }
    if (!prompt) return undefined;
    const pick = await vscode.window.showQuickPick(owners.map(root => ({ label: vscode.workspace.asRelativePath(root), description: root.fsPath, root })), { placeHolder: "Choose the model that owns this include" }, token);
    if (token?.isCancellationRequested) return undefined;
    if (this.selectedOwners.get(key) !== selection) return undefined;
    if (!pick) throw new CommandCancelled();
    await this.applyOwner(document, pick.root);
    await this.reportDocumentContextOutcome();
    return pick.root;
  }
  private ownsDocument(info: ModelInfo, document: vscode.Uri): boolean {
    const equal = (value: string): boolean => sameUri(vscode.Uri.parse(value), document);
    return equal(info.root_uri) || info.browsing_documents?.some(equal) === true ||
      info.source_browsing === true && equal(info.document_uri) || this.executesDocument(info, document);
  }
  private executesDocument(info: ModelInfo, document: vscode.Uri): boolean {
    const equal = (value: string): boolean => sameUri(vscode.Uri.parse(value), document);
    return equal(info.root_uri) || info.statements.some(row => row.segments.some(segment => equal(segment.uri))) ||
      info.related_files.some(row => row.kind === "include" && row.path && sameUri(vscode.Uri.file(row.path), document));
  }
  private async openIncludeLink(key: unknown): Promise<void> {
    const unavailable = (): never => refuse("This include link expired or its model owner changed. Request the link from the current source again.");
    if (typeof key !== "string") { unavailable(); return; }
    const link = this.includeLinks.get(key);
    if (!link || link.instance !== this.instance) { unavailable(); return; }
    const source = vscode.workspace.textDocuments.find(document => sameUri(document.uri, link.source));
    if (!source || source.version !== link.version) { unavailable(); return; }
    const sourceKey = source.uri.toString();
    const editor = vscode.window.activeTextEditor;
    const invocationOwner = this.selectedOwners.get(sourceKey);
    const invocationOwned = this.ownedDocuments.has(sourceKey);
    const currentInput = (): boolean => !this.closed && this.instance === link.instance &&
      source.version === link.version && vscode.window.activeTextEditor === editor;
    const root = await this.rootForDocument(source, true);
    if (!root || !currentInput()) { unavailable(); return; }
    // A root picker may intentionally choose an owner. Retain its accepted
    // outcome, then refuse later context changes during validation or loading.
    const owner = this.selectedOwners.get(sourceKey);
    const owned = this.ownedDocuments.has(sourceKey);
    if (sameUri(root, source.uri)
      ? owner !== invocationOwner || owned !== invocationOwned
      : owner !== root.toString() || !owned) { unavailable(); return; }
    const currentContext = (): boolean => currentInput() &&
      this.selectedOwners.get(sourceKey) === owner && this.ownedDocuments.has(sourceKey) === owned;
    // Re-resolve against the selected configuration and include paths. A source
    // version alone cannot prove a link after an analysis-only choice changes.
    const links: unknown = await this.client?.sendRequest("textDocument/documentLink", {textDocument:{uri:source.uri.toString()}});
    if (!currentContext() || !Array.isArray(links) || !links.some(candidate => record(candidate) && typeof candidate.target === "string" && sameUri(vscode.Uri.parse(candidate.target), link.target))) { unavailable(); return; }
    const target = await vscode.workspace.openTextDocument(link.target);
    if (!currentContext()) { unavailable(); return; }
    if (target.languageId !== "dynare") await vscode.languages.setTextDocumentLanguage(target, "dynare");
    if (!currentContext()) { unavailable(); return; }
    await this.openSource(link.target, root, currentContext);
  }
  async modelForDocument(document: vscode.TextDocument, prompt = false): Promise<ModelSnapshot | undefined> {
    const root = await this.rootForDocument(document, prompt);
    return root ? this.modelInfo(root, document.uri) : undefined;
  }
  selectOwner(document: vscode.Uri, root: vscode.Uri): void {
    this.rememberOwner(document, root, true);
  }
  /** Accept the same guarded choice in the server before changing the view. */
  async applyOwner(document: vscode.TextDocument, root: vscode.Uri, info?: ModelSnapshot, guard: () => boolean = () => true): Promise<void> {
    await this.applyDocumentContext(document, root, () => this.selectOwner(document.uri, root), info, guard);
  }
  async treatDocumentAsRoot(document: vscode.TextDocument): Promise<void> {
    if (!isAnalysisDocument(document) || !isRootUri(document.uri)) refuse("Open a .mod or .dyn file before treating it as a model root.");
    await this.applyDocumentContext(document, document.uri, () => this.rememberStandalone(document.uri));
    await this.reportDocumentContextOutcome();
  }
  /** The explicit command owns the explanation for older configured engines. */
  async reportDocumentContextOutcome(): Promise<void> {
    if (!this.documentContextSupported) await vscode.window.showInformationMessage(
      "The model choice applies to model information and macro preview. Editor diagnostics still use the engine's automatic context because the selected engine cannot accept owner choices. Use the bundled binary or update dynare.serverPath to apply this choice to diagnostics too.");
  }
  private async applyDocumentContext(document: vscode.TextDocument, root: vscode.Uri, accepted: () => void, proof?: ModelSnapshot, guard: () => boolean = () => true): Promise<void> {
    await this.ensureStarted();
    if (this.closed || document.isClosed || !guard()) refuse("The document or source changed. Request the model action again.");
    if (!this.documentContextSupported) {
      this.log("The selected engine cannot apply owner choices to editor diagnostics; this choice applies to model information and macro preview only.");
      accepted();
      return;
    }
    const key = document.uri.toString(), version = document.version, instance = this.instance;
    const generation = (this.contextGenerations.get(key) ?? 0) + 1;
    this.contextGenerations.set(key, generation);
    const lifecycle = this.contextLifecycles.get(key) ?? 0;
    const sameLifecycle = (): boolean => !this.closed && !document.isClosed && this.instance === instance && (this.contextLifecycles.get(key) ?? 0) === lifecycle;
    const current = (): boolean => sameLifecycle() && this.contextGenerations.get(key) === generation && document.version === version && guard();
    const info = proof ?? await this.modelInfo(root, document.uri, true);
    if (!current() || !info || info.document_version !== version || info.client_instance !== instance) refuse("The document or model changed. Choose its model context again.");
    await this.execute("dynare/setDocumentContext", [{ document_uri: key, document_version: version, root_uri: root.toString(), expected_revision: info.revision }]);
    if (!sameLifecycle() || (this.acceptedContextGenerations.get(key) ?? 0) > generation) refuse("A newer model choice or a closed or restarted document replaced this choice. Run the model action again.");
    const actionCurrent = current();
    const newerAttempt = this.contextGenerations.get(key) !== generation;
    this.acceptedContextGenerations.set(key, generation);
    accepted();
    this.scheduleSymbolRefresh();
    if (newerAttempt) refuse("The model choice was applied, but its pending action stopped because another model choice was requested. Complete that choice or run the action again.");
    if (!actionCurrent) refuse("The model choice was applied, but the pending action stopped because the source or model changed. Run the action again.");
  }
  private clearDocumentContext(key: string): void {
    this.contextGenerations.set(key, (this.contextGenerations.get(key) ?? 0) + 1);
    this.contextLifecycles.set(key, (this.contextLifecycles.get(key) ?? 0) + 1);
    this.acceptedContextGenerations.delete(key);
    this.selectedOwners.delete(key); this.explicitOwnerDocuments.delete(key); this.ownedDocuments.delete(key); this.standaloneDocuments.delete(key);
  }
  private clearDocumentContexts(): void {
    for (const key of new Set([...this.selectedOwners.keys(), ...this.standaloneDocuments, ...this.contextGenerations.keys()])) this.clearDocumentContext(key);
  }
  private rememberOwner(document: vscode.Uri, root: vscode.Uri, explicit: boolean): void {
    if (document.toString() === root.toString()) return;
    const key = document.toString();
    const changed = this.selectedOwners.get(key) !== root.toString() || this.explicitOwnerDocuments.has(key) !== explicit || this.standaloneDocuments.has(key);
    this.standaloneDocuments.delete(key);
    this.selectedOwners.set(key, root.toString()); this.ownedDocuments.add(key);
    if (explicit) this.explicitOwnerDocuments.add(key); else this.explicitOwnerDocuments.delete(key);
    if (changed) this.changed.fire();
  }
  private rememberStandalone(document: vscode.Uri): void {
    const key = document.toString();
    this.selectedOwners.delete(key); this.explicitOwnerDocuments.delete(key); this.ownedDocuments.delete(key); this.standaloneDocuments.add(key);
    this.changed.fire(); this.scheduleRootRefresh();
  }
  treatAsRoot(document: vscode.Uri): void { this.clearDocumentContext(document.toString()); this.rememberStandalone(document); }
  async openLocation(location: { uri: string; range: { start: { line: number; character: number }; end: { line: number; character: number } } }, root?: vscode.Uri, guard?: NavigationGuard, options?: { viewColumn?: vscode.ViewColumn; reuseOpen?: boolean }): Promise<void> {
    const uri = vscode.Uri.parse(location.uri);
    const range = new vscode.Range(location.range.start.line, location.range.start.character, location.range.end.line, location.range.end.character);
    await this.openSource(uri, root, guard, range, options);
  }
  private async openSource(uri: vscode.Uri, root?: vscode.Uri, guard?: NavigationGuard, selection?: vscode.Range, options?: { viewColumn?: vscode.ViewColumn; reuseOpen?: boolean }): Promise<void> {
    if (!["file", "untitled"].includes(uri.scheme)) throw new Error("This source location is unavailable to native analysis.");
    if (root) await this.ensureStarted();
    const document = await vscode.workspace.openTextDocument(uri);
    const decision = guard ? await guard(document) : true;
    // An asynchronous proof may expire before this continuation can reveal it.
    if (typeof decision === "boolean" ? !decision : decision.isCurrent() !== true) {
      refuse("The model or source changed while loading this location. Request the source jump again.");
    }
    const epoch = this.epoch, instance = this.instance, editor = vscode.window.activeTextEditor, version = document.version;
    const current = (): boolean => !document.isClosed && document.version === version && this.epoch === epoch && this.instance === instance &&
      vscode.window.activeTextEditor === editor && (typeof decision === "boolean" ? decision : decision.isCurrent());
    if (root && !sameUri(document.uri, root)) await this.applyOwner(document, root, undefined, current);
    if (!current()) refuse("The model or source changed while preparing this location. Request the source jump again.");
    const viewColumn = options?.reuseOpen ? reuseOpenViewColumn(document.uri) : options?.viewColumn;
    await vscode.window.showTextDocument(document, { viewColumn, selection });
  }
  async failure(message: string): Promise<void> {
    this.log(message);
    if (this.reportedFailure === message) return;
    this.reportedFailure = message;
    const action = await vscode.window.showErrorMessage(message, "Show Output", "Use bundled binary", "Open Settings", "Open Help");
    if (action === "Show Output") this.output.show();
    else if (action === "Use bundled binary") await vscode.workspace.getConfiguration("dynare").update("serverPath", "", vscode.ConfigurationTarget.Global);
    else if (action === "Open Settings") await vscode.commands.executeCommand("workbench.action.openSettings", "@ext:CoconutWater.dygnosis dynare.serverPath");
    else if (action === "Open Help") await vscode.commands.executeCommand("dygnosis.openHelp", "troubleshoot");
  }
  shutdown(): Promise<void> {
    if (this.shutdownPromise) return this.shutdownPromise;
    this.closed = true;
    ++this.startGeneration;
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    if (this.symbolTimer) clearTimeout(this.symbolTimer);
    this.invalidateLifecycle();
    this.shutdownPromise = this.lifecycle.shutdown().then(() => {
      for (const disposable of this.disposables) disposable.dispose();
      for (const subscription of this.instanceSubscriptions) subscription.dispose();
      for (const watcher of this.dependencyWatchers.values()) watcher.dispose();
      this.dependencyWatchers.clear(); this.rootCandidates.clear();
      this.includeLinks.clear();
    });
    return this.shutdownPromise;
  }
  dispose(): void { void this.shutdown(); }
}
function textTabUris(input: vscode.Tab["input"]): vscode.Uri[] {
  if (input instanceof vscode.TabInputText) return [input.uri];
  if (input instanceof vscode.TabInputTextDiff) return [input.original, input.modified];
  return [];
}
/** Prefer a visible or hidden text tab after async validation; otherwise open beside. */
function reuseOpenViewColumn(uri: vscode.Uri): vscode.ViewColumn {
  const visible = vscode.window.visibleTextEditors.filter(editor => sameUri(editor.document.uri, uri));
  const activeColumn = vscode.window.activeTextEditor?.viewColumn;
  const inActive = visible.find(editor => editor.viewColumn === activeColumn);
  if (inActive?.viewColumn !== undefined) return inActive.viewColumn;
  if (visible[0]?.viewColumn !== undefined) return visible[0].viewColumn;
  for (const group of vscode.window.tabGroups.all) {
    for (const tab of group.tabs) {
      const input = tab.input;
      if (input instanceof vscode.TabInputText && sameUri(input.uri, uri)) return group.viewColumn;
    }
  }
  return vscode.ViewColumn.Beside;
}
function requireDirectory(filename: string): string {
  const slash = Math.max(filename.lastIndexOf("/"), filename.lastIndexOf("\\"));
  return filename.slice(0, slash);
}
