import { spawn, ChildProcess } from "node:child_process";
import * as vscode from "vscode";
import {
  ClientCapabilities, DocumentSymbolRequest, ExecuteCommandRequest, FeatureState, InitializeParams,
  LanguageClient, Middleware, StaticFeature, State,
} from "vscode-languageclient/node";
import { Binary, resolveBinary } from "./binary";
import { ClientLifecycle, ManagedClient } from "./lifecycle";
import { configurationSnapshot } from "./settings";
import { ModelInfo, parseModelInfo, record, timingModifiers, tokenRoles } from "./protocol";

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
  experimental.dygnosis = { modelInfo: { schema_version: 1 }, modelInfoChanged: true, configuration: { schema_version: 1 },
    compareModels: { navigation_schema_version: 1 }, effectivePreview: { navigation_schema_version: 1, dependency_candidates: true },
    projectStatusChanged: true };
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
    this.client = new LanguageClient("dygnosis", "Dygnosis", () => {
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
    // Killing our child also rejects initialize if the server never answered.
    this.process?.kill();
    this.disposing = this.client.dispose(1500).catch(() => undefined);
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
export interface InputInvalidation { root?: string; reason: "input" | "file"; uri?: string }

/** Shared service for status, view, tint, lenses, diagnostics and previews. */
export class DygnosisClient implements vscode.Disposable {
  readonly output = vscode.window.createOutputChannel("Dygnosis");
  readonly middleware: Middleware = {};
  private readonly lifecycle = new ClientLifecycle<ClientProcess>();
  private readonly changed = new vscode.EventEmitter<void>();
  readonly onDidChange = this.changed.event;
  private readonly modelInfoUpdated = new vscode.EventEmitter<ModelSnapshot>();
  readonly onDidUpdateModelInfo = this.modelInfoUpdated.event;
  private readonly invalidated = new vscode.EventEmitter<InputInvalidation>();
  readonly onDidInvalidate = this.invalidated.event;
  private readonly disposables: vscode.Disposable[] = [this.output, this.changed, this.invalidated, this.modelInfoUpdated];
  private readonly cache = new Map<string, CachedRequest>();
  private readonly freshTails = new Map<string, Promise<void>>();
  private readonly cancelFreshRequests = new Set<() => void>();
  private readonly infos = new Map<string, ModelInfo>();
  private readonly selectedOwners = new Map<string, string>();
  private readonly explicitOwnerDocuments = new Set<string>();
  private readonly ownedDocuments = new Set<string>();
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
        if (!link.target || !isRootUri(link.target)) return link;
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
    this.disposables.push(vscode.commands.registerCommand("dygnosis.openIncludeLink", (key: unknown) => this.openIncludeLink(key)));
    const watcher = vscode.workspace.createFileSystemWatcher("**/*.{mod,dyn,inc}");
    this.disposables.push(this.watch(watcher),
      vscode.workspace.onDidOpenTextDocument(document => {
        if (isAnalysisDocument(document)) void this.ensureStarted();
        else if (document.languageId === "dynare" && !document.uri.scheme.startsWith("dygnosis-"))
          this.output.appendLine(`Grammar only for ${document.uri.scheme}: documents; native analysis requires file or untitled.`);
      }),
      vscode.workspace.onDidChangeTextDocument(event => {
        if (event.contentChanges.length && isAnalysisDocument(event.document)) {
          this.pruneSourceLinks(event.document.uri, event.document.version);
          this.invalidate();
        }
      }),
      vscode.workspace.onDidCloseTextDocument(document => {
        if (isAnalysisDocument(document)) { this.explicitOwnerDocuments.delete(document.uri.toString()); this.pruneSourceLinks(document.uri); this.invalidate(); }
      }),
      vscode.workspace.onDidChangeWorkspaceFolders(() => { this.selectedOwners.clear(); this.explicitOwnerDocuments.clear(); this.ownedDocuments.clear(); void this.sendSettings(); }),
      vscode.workspace.onDidChangeConfiguration(event => {
        if (!event.affectsConfiguration("dynare")) return;
        if (event.affectsConfiguration("dynare.serverPath")) void this.restart();
        else void this.sendSettings();
      }),
    );
  }
  get client(): LanguageClient | undefined { return this.lifecycle.client?.client; }
  get currentInstance(): number { return this.instance; }
  get supportsModelInfo(): boolean { return this.modelInfoSupported; }
  private pruneSourceLinks(source: vscode.Uri, version?: number): void {
    for (const [key, link] of this.includeLinks) if (sameUri(link.source, source) && link.version !== version) this.includeLinks.delete(key);
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
  invalidate(root?: string, file?: vscode.Uri): void {
    ++this.epoch;
    // Every in-flight snapshot uses the global epoch. Release its fresh
    // callers immediately; a server ignoring cancellation must not hold a queue.
    for (const cancel of this.cancelFreshRequests) cancel();
    for (const [key, request] of this.cache) {
      if (!root || key.startsWith(`${root}\n`)) { request.cancellation.cancel(); request.cancellation.dispose(); this.cache.delete(key); }
    }
    if (root) this.infos.delete(root); else this.infos.clear();
    this.invalidated.fire(file ? { root, reason: "file", uri: file.toString() } : { root, reason: "input" });
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
      void Promise.all([...roots].map(root => this.modelInfo(vscode.Uri.parse(root))));
    }, 75);
  }
  async ensureStarted(): Promise<void> {
    if (this.closed) return;
    if (this.starting) return this.starting;
    if (this.client) return;
    this.starting = this.start().finally(() => { this.starting = undefined; });
    return this.starting;
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
      await this.lifecycle.replace(() => create(binary, this.output, this.middleware, configurationSnapshot(this.log), params => { this.lastInitializeParams = params; }), async managed => {
        this.reportedFailure = undefined;
        const client = managed.client;
        ++this.instance;
        await this.reconcileStartup(client);
        if (this.closed || generation !== this.startGeneration || client !== this.client) return;
        this.log(`Engine: ${client.initializeResult?.serverInfo?.name ?? "unknown"} ${client.initializeResult?.serverInfo?.version ?? "unknown version"}`);
        const experimental: unknown = client.initializeResult?.capabilities.experimental;
        this.modelInfoSupported = record(experimental) && record(experimental.dygnosis) &&
          record(experimental.dygnosis.modelInfo) && experimental.dygnosis.modelInfo.schema_version === 1;
        this.instanceSubscriptions.push(client.onNotification("dynare/modelInfoChanged", (value: unknown) => {
          if (client === this.client && record(value) && value.schema_version === 1 && typeof value.root_uri === "string") this.invalidate(value.root_uri);
        }), client.onDidChangeState(event => {
          if (client === this.client && event.newState === State.Stopped) { this.modelInfoSupported = false; this.invalidate(); }
        }));
        this.refreshSymbols();
        this.invalidate();
        if (!this.modelInfoSupported) void this.failure("This engine does not support the Dynare model view. Update dynare.serverPath or use the bundled binary.");
      });
    } catch (error) { if (!this.closed && generation === this.startGeneration) await this.failure(`Dygnosis could not start. Reinstall the extension or check dynare.serverPath. ${String(error)}`); }
  }
  async restart(): Promise<void> {
    if (this.closed) return;
    this.modelInfoSupported = false;
    this.symbolRegistration = false;
    this.invalidate();
    // start() replaces and disposes the prior client, including a pending start.
    const replacement = this.start();
    this.starting = replacement;
    await replacement;
    if (this.starting === replacement) this.starting = undefined;
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
    return client.sendRequest(ExecuteCommandRequest.type, { command, arguments: args }, token);
  }
  async modelInfo(root: vscode.Uri, document = root, fresh = false, token?: vscode.CancellationToken): Promise<ModelSnapshot | undefined> {
    if (token?.isCancellationRequested) return undefined;
    await this.ensureStarted();
    if (!this.modelInfoSupported || this.closed || token?.isCancellationRequested) return undefined;
    return fresh ? this.freshModelInfo(root, document, token) : this.requestModelInfo(root, document);
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
    const tail = task.then(() => {});
    this.freshTails.set(key, tail);
    void result.then(() => {
      this.cancelFreshRequests.delete(cancel);
      callerSubscription?.dispose();
    });
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
      this.infos.set(rootKey, info);
      this.watchDependencies(info);
      const snapshot = { ...info, client_instance: this.instance };
      this.modelInfoUpdated.fire(snapshot);
      return snapshot;
    }).catch((error: unknown) => {
      if (!cancellation.token.isCancellationRequested) { this.log(String(error)); void this.failure(String(error)); }
      return undefined;
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
    return [...this.infos.values()].filter(info => this.ownsDocument(info, document))
      .map(info => vscode.Uri.parse(info.root_uri));
  }
  /** Read proven chosen context without requests, discovery, or a picker. */
  chosenRootForDocument(document: vscode.TextDocument): vscode.Uri | undefined {
    if (this.closed || document.isClosed || !isAnalysisDocument(document)) return undefined;
    const key = document.uri.toString(), selected = this.selectedOwners.get(key);
    if (selected) {
      if (!this.explicitOwnerDocuments.has(key)) return undefined;
      const info = this.infos.get(selected);
      return info && this.ownsDocument(info, document.uri) ? vscode.Uri.parse(selected) : undefined;
    }
    return isRootUri(document.uri) && !this.ownedDocuments.has(key) ? document.uri : undefined;
  }
  async ownerChoices(document: vscode.TextDocument): Promise<vscode.Uri[]> {
    const roots = new Set(vscode.workspace.textDocuments.filter(doc => isRootUri(doc.uri) && !this.ownedDocuments.has(doc.uri.toString())).map(doc => doc.uri.toString()));
    for (const root of this.selectedOwners.values()) roots.add(root);
    await Promise.all([...roots].map(root => this.modelInfo(vscode.Uri.parse(root))));
    return this.knownOwners(document.uri);
  }
  async rootForDocument(document: vscode.TextDocument, prompt = false): Promise<vscode.Uri | undefined> {
    if (!isAnalysisDocument(document)) return undefined;
    const key = document.uri.toString(), selected = this.selectedOwners.get(key);
    if (selected) {
      const root = vscode.Uri.parse(selected);
      const info = await this.modelInfo(root);
      if (this.selectedOwners.get(key) !== selected) return undefined;
      if (!info) return undefined;
      if (this.ownsDocument(info, document.uri)) return root;
      this.selectedOwners.delete(key);
      this.explicitOwnerDocuments.delete(key);
    }
    if (isRootUri(document.uri) && !this.ownedDocuments.has(key)) return document.uri;
    // Discover owners only from available models, never analyze an include as a root.
    const selection = this.selectedOwners.get(key);
    const owners = await this.ownerChoices(document);
    if (this.selectedOwners.get(key) !== selection) return undefined;
    if (owners.length === 1) { this.rememberOwner(document.uri, owners[0], false); return owners[0]; }
    if (!prompt || owners.length === 0) return undefined;
    const pick = await vscode.window.showQuickPick(owners.map(root => ({ label: vscode.workspace.asRelativePath(root), description: root.fsPath, root })), { placeHolder: "Choose the model that owns this include" });
    if (this.selectedOwners.get(key) !== selection) return undefined;
    if (pick) this.selectOwner(document.uri, pick.root);
    return pick?.root;
  }
  private ownsDocument(info: ModelInfo, document: vscode.Uri): boolean {
    const equal = (value: string): boolean => sameUri(vscode.Uri.parse(value), document);
    return equal(info.root_uri) || info.statements.some(row => row.segments.some(segment => equal(segment.uri))) ||
      info.related_files.some(row => row.kind === "include" && row.path && sameUri(vscode.Uri.file(row.path), document));
  }
  private async openIncludeLink(key: unknown): Promise<void> {
    if (typeof key !== "string") return;
    const link = this.includeLinks.get(key);
    if (!link || link.instance !== this.instance) return;
    const source = vscode.workspace.textDocuments.find(document => sameUri(document.uri, link.source));
    if (!source || source.version !== link.version) return;
    const sourceKey = source.uri.toString();
    const editor = vscode.window.activeTextEditor;
    const invocationOwner = this.selectedOwners.get(sourceKey);
    const invocationOwned = this.ownedDocuments.has(sourceKey);
    const currentInput = (): boolean => !this.closed && this.instance === link.instance &&
      source.version === link.version && vscode.window.activeTextEditor === editor;
    const root = await this.rootForDocument(source, true);
    if (!root || !currentInput()) return;
    // A root picker may intentionally choose an owner. Retain its accepted
    // outcome, then refuse later context changes during validation or loading.
    const owner = this.selectedOwners.get(sourceKey);
    const owned = this.ownedDocuments.has(sourceKey);
    if (sameUri(root, source.uri)
      ? owner !== invocationOwner || owned !== invocationOwned
      : owner !== root.toString() || !owned) return;
    const currentContext = (): boolean => currentInput() &&
      this.selectedOwners.get(sourceKey) === owner && this.ownedDocuments.has(sourceKey) === owned;
    const info = await this.modelInfo(root, source.uri, true);
    if (!info || !currentContext() || info.client_instance !== link.instance || !this.ownsDocument(info, link.target)) return;
    await this.openSource(link.target, root, currentContext);
  }
  async modelForDocument(document: vscode.TextDocument, prompt = false): Promise<ModelSnapshot | undefined> {
    const root = await this.rootForDocument(document, prompt);
    return root ? this.modelInfo(root, document.uri) : undefined;
  }
  selectOwner(document: vscode.Uri, root: vscode.Uri): void {
    this.rememberOwner(document, root, true);
  }
  private rememberOwner(document: vscode.Uri, root: vscode.Uri, explicit: boolean): void {
    if (document.toString() === root.toString()) return;
    const key = document.toString();
    this.selectedOwners.set(key, root.toString()); this.ownedDocuments.add(key);
    if (explicit) this.explicitOwnerDocuments.add(key); else this.explicitOwnerDocuments.delete(key);
    this.changed.fire();
  }
  treatAsRoot(document: vscode.Uri): void { this.selectedOwners.delete(document.toString()); this.explicitOwnerDocuments.delete(document.toString()); this.ownedDocuments.delete(document.toString()); this.changed.fire(); this.scheduleRootRefresh(); }
  async openLocation(location: { uri: string; range: { start: { line: number; character: number }; end: { line: number; character: number } } }, root?: vscode.Uri, guard?: NavigationGuard, options?: { viewColumn?: vscode.ViewColumn; reuseOpen?: boolean }): Promise<void> {
    const uri = vscode.Uri.parse(location.uri);
    const range = new vscode.Range(location.range.start.line, location.range.start.character, location.range.end.line, location.range.end.character);
    await this.openSource(uri, root, guard, range, options);
  }
  private async openSource(uri: vscode.Uri, root?: vscode.Uri, guard?: NavigationGuard, selection?: vscode.Range, options?: { viewColumn?: vscode.ViewColumn; reuseOpen?: boolean }): Promise<void> {
    if (!["file", "untitled"].includes(uri.scheme)) throw new Error("This source location is unavailable to native analysis.");
    const document = await vscode.workspace.openTextDocument(uri);
    const decision = guard ? await guard(document) : true;
    // An asynchronous proof may expire before this continuation can reveal it.
    if (typeof decision === "boolean" ? !decision : decision.isCurrent() !== true) return;
    if (root) this.selectOwner(document.uri, root);
    const viewColumn = options?.reuseOpen ? reuseOpenViewColumn(document.uri) : options?.viewColumn;
    const { reuseOpen: _reuseOpen, ...rest } = options ?? {};
    await vscode.window.showTextDocument(document, { ...rest, viewColumn, selection });
  }
  async failure(message: string): Promise<void> {
    this.log(message);
    if (this.reportedFailure === message) return;
    this.reportedFailure = message;
    const action = await vscode.window.showErrorMessage(message, "Show Output", "Use bundled binary", "Open Settings", "Open Help");
    if (action === "Show Output") this.output.show();
    else if (action === "Use bundled binary") await vscode.workspace.getConfiguration("dynare").update("serverPath", "", vscode.ConfigurationTarget.Global);
    else if (action === "Open Settings") await vscode.commands.executeCommand("workbench.action.openSettings", "@ext:dygnosis.dygnosis dynare.serverPath");
    else if (action === "Open Help") await vscode.commands.executeCommand("dygnosis.openHelp", "troubleshoot");
  }
  shutdown(): Promise<void> {
    if (this.shutdownPromise) return this.shutdownPromise;
    this.closed = true;
    ++this.startGeneration;
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    if (this.symbolTimer) clearTimeout(this.symbolTimer);
    this.invalidate();
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
function sameUri(left: vscode.Uri, right: vscode.Uri): boolean {
  if (left.scheme !== right.scheme) return false;
  if (left.scheme === "file" && process.platform === "win32") return left.fsPath.toLowerCase() === right.fsPath.toLowerCase();
  return left.toString() === right.toString();
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
