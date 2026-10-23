const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");
const path = require("node:path");
const { minimatch } = require("minimatch");
class Disposable {
  constructor(callback = () => {}) { this.callback = callback; }
  dispose() { this.callback(); this.callback = () => {}; }
  static from(...values) { return new Disposable(() => values.forEach(value => value.dispose())); }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
  dispose() { this.listeners.clear(); }
}
class Uri {
  constructor(value) { this.value = value; const parsed = new URL(value); this.scheme = parsed.protocol.slice(0, -1); this.path = parsed.pathname; this.fsPath = decodeURIComponent(parsed.pathname); this.query = parsed.search.slice(1); }
  toString() { return this.value; }
  static parse(value) { return new Uri(value); }
  static file(value) { return new Uri(`file://${value.replaceAll("\\", "/")}`); }
}
function deferred() { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; }
class CancellationTokenSource {
  cancelled = new Emitter();
  token = { isCancellationRequested: false, onCancellationRequested: this.cancelled.event };
  cancel() { if (!this.token.isCancellationRequested) { this.token.isCancellationRequested = true; this.cancelled.fire(); } }
  dispose() { this.cancelled.dispose(); }
}
function matches(pattern, value) {
  const relative = typeof pattern === "string" ? value.fsPath.replace(/^\/+/, "") : path.relative(pattern.base.fsPath, value.fsPath).replaceAll("\\", "/");
  return minimatch(relative, typeof pattern === "string" ? pattern : pattern.pattern, { dot: true });
}
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
class TabInputText { constructor(uri) { this.uri = uri; } }
class TabInputTextDiff { constructor(original, modified) { this.original = original; this.modified = modified; } }
const vscode = {
  Disposable, EventEmitter: Emitter, Uri, TabInputText, TabInputTextDiff, ViewColumn: { Beside: 2, One: 1 },
  DocumentLink: class { constructor(range, target) { this.range = range; this.target = target; } },
  Range: class { constructor(...coordinates) { this.coordinates = coordinates; } },
  RelativePattern: class { constructor(base, pattern) { this.base = base; this.pattern = pattern; } }, CancellationTokenSource,
  window: {
    showInformationMessage: message => { host.notifications.push(message); return Promise.resolve(); },
    get activeTextEditor() { return host.editor; },
    get visibleTextEditors() { return host.visibleEditors ?? (host.editor ? [host.editor] : []); },
    tabGroups: { get all() { return host.tabGroups ?? []; }, onDidChangeTabs: listener => host.tabsChanged.event(listener) },
    createOutputChannel: () => ({ appendLine() {}, append() {}, show() {}, dispose() {} }),
    showErrorMessage: () => Promise.resolve(undefined), showQuickPick: items => { host.picks.push(items); return Promise.resolve(host.pick ? host.pick(items) : items[0]); },
    showTextDocument: (document, options) => { host.opened.push(document.uri.toString()); host.reveals.push({ document, options }); return Promise.resolve({ document }); },
  },
  commands: { registerCommand: (name, callback) => { host.commands.set(name, callback); return new Disposable(() => host.commands.delete(name)); } },
  workspace: {
    get textDocuments() { return host.documents; },
    get workspaceFolders() { return host.folders; },
    getConfiguration: () => ({ get: (key, fallback) => host.settings[key] ?? fallback, inspect: key => ({ globalValue: host.settings[key] }) }),
    createFileSystemWatcher: pattern => {
      const subscribe = event => listener => event(value => { if (matches(pattern, value)) listener(value); });
      const watcher = { pattern, onDidCreate: subscribe(host.created.event), onDidChange: subscribe(host.disk.event), onDidDelete: subscribe(host.deleted.event), dispose() {} };
      host.watchers.push(watcher); return watcher;
    },
    onDidOpenTextDocument: callback => host.open.event(callback),
    onDidChangeTextDocument: callback => host.edit.event(callback),
    onDidCloseTextDocument: callback => host.close.event(callback),
    onDidChangeWorkspaceFolders: callback => host.folder.event(callback),
    onDidChangeConfiguration: callback => host.config.event(callback),
    openTextDocument: uri => { const document = { uri, languageId: "dynare", version: 1 }; host.documents.push(document); host.open.fire(document); return Promise.resolve(document); },
    asRelativePath: uri => uri.path,
  },
};
const languageClient = { DocumentSymbolRequest: { method: "textDocument/documentSymbol" }, RenameRequest: { method: "textDocument/rename" }, ExecuteCommandRequest: { type: "workspace/executeCommand" }, State: { Stopped: 1, Running: 2, Starting: 3 } };
const originalLoad = Module._load;
Module._load = function(id, ...args) { return id === "vscode" ? vscode : id === "vscode-languageclient/node" ? languageClient : originalLoad.call(this, id, ...args); };
const { DygnosisClient, extendCapabilities } = require(compiledModule("client"));
const { runCommand } = require(compiledModule("refusal"));
Module._load = originalLoad;
function reset() {
  host = { documents: [], folders: [{ uri: Uri.parse("file:///workspace"), name: "workspace" }], settings: {}, commands: new Map(), opened: [], reveals: [], managed: [], watchers: [], notifications: [], picks: [], symbols: 0,
    visibleEditors: undefined, tabGroups: [] };
  for (const name of ["created", "disk", "deleted", "open", "edit", "close", "folder", "config", "tabsChanged"]) host[name] = new Emitter();
}
function model(root = "file:///workspace/root.mod", related = []) {
  return { schema_version: 1, root_uri: root, document_uri: root, document_version: 1, revision: "one", complete: false,
    owner_roots: [], statements: [], declarations: [], equations: [], related_files: related, block_categories: [], first_model_anchor: null };
}
function service(options = {}) {
  return new DygnosisClient({}, {
    resolve: options.resolve ?? (() => Promise.resolve({ path: "engine", override: true, version: "one" })),
    create: (_binary, _output, _middleware, settings, capture) => {
      const initialized = deferred();
      const client = {
        initializeResult: { serverInfo: { name: "dygnosis", version: "one" }, capabilities: {
          experimental: { dygnosis: { modelInfo: { schema_version: 1 }, ...(options.analysis ? {analysisConfiguration:{schema_version:1}} : {}), ...(options.documentContext ? { documentContext: { schema_version: 1, command: "dynare/setDocumentContext" } } : {}) } }, documentSymbolProvider: true,
          executeCommandProvider: { commands: ["dynare/modelInfo", "dynare/showEffectiveModel", ...(options.documentContext ? ["dynare/setDocumentContext"] : [])] },
        } },
        sendNotification: (method, params) => { host.notifications.push({ method, params }); return options.notify?.(method, params) ?? Promise.resolve(); },
        onNotification: (_method, listener) => { client.notify = listener; return new Disposable(); },
        onDidChangeState: listener => { client.state = listener; return new Disposable(); },
        getFeature: request => ({ clear() { (host.clearedFeatures ??= []).push(request); }, unregister() {}, register() { ++host.symbols; } }),
        sendRequest: (method, params, token) => _middleware.sendRequest(method, params, token, () => Promise.resolve(options.request
          ? options.request(method, params, token) : method === "textDocument/documentLink" ? host.links?.(params) ?? [] : model())),
      };
      const managed = { client, disposed: false,
        start: () => { capture({ workspaceFolders: host.folders.map(folder => ({ uri: folder.uri.toString(), name: folder.name })), capabilities: {}, initializationOptions: settings }); if (!options.pending) initialized.resolve(); return initialized.promise; },
        dispose: () => { managed.disposed = true; initialized.resolve(); return Promise.resolve(); }, initialized,
      };
      host.managed.push(managed); return managed;
    },
  });
}

test("Outline presentation changes retain model snapshots and still refresh symbols", async () => {
  // Guards a filter click discarding checked model information and starting analysis requests.
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod");
  host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  try {
    const before = await client.modelInfo(root), symbols = host.symbols;
    let invalidations = 0;
    client.onDidInvalidate(() => { ++invalidations; });
    host.settings["outline.activity"] = "inactive";
    host.config.fire({ affectsConfiguration: key => ["dynare", "dynare.outline", "dynare.outline.activity"].includes(key) });
    await flush();
    assert.equal(await client.modelInfo(root), before);
    assert.equal(invalidations, 0);
    assert.ok(host.symbols > symbols);
    assert.equal(host.notifications.at(-1).params.settings.dynare.configuration.loose.outline.activity, "inactive");
  } finally { await client.shutdown(); }
});

test("editing one root retains an independent cache and cancels unproven pending inputs", async () => {
  // Guards an unrelated edit cancelling a valid response or checking every open model again.
  reset(); const client = service(); await client.ensureStarted();
  const roots = ["a", "b", "c"].map(name => Uri.parse(`file:///workspace/${name}.mod`));
  host.documents.push(...roots.map(uri => ({ uri, languageId: "dynare", version: 1 })));
  const pending = deferred(), calls = [];
  client.client.sendRequest = (_method, params) => {
    const root = params.arguments[0].root_uri; calls.push(root);
    if (root === roots[2].toString()) return pending.promise;
    return Promise.resolve({ ...model(root), dependency_candidates: [root] });
  };
  try {
    await client.modelInfo(roots[0]); const retained = await client.modelInfo(roots[1]);
    const inFlight = client.modelInfo(roots[2]); await flush();
    host.documents[0].version = 2;
    host.edit.fire({ document: host.documents[0], contentChanges: [{ text: "var y;" }] });
    pending.resolve({ ...model(roots[2].toString()), dependency_candidates: [roots[2].toString()] });
    assert.equal(await client.modelInfo(roots[1]), retained);
    // An input with no dependency proof is conservatively cancelled, even on another root.
    assert.equal(await inFlight, undefined);
    assert.equal(calls.filter(root => root === roots[1].toString()).length, 1);
  } finally { await client.shutdown(); }
});

test("an unrelated edit preserves a fresh proof with known dependency coverage", async () => {
  // Guards root-scoped invalidation still cancelling every serialized fresh caller.
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), pending = deferred();
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, { uri: b, languageId: "dynare", version: 1 });
  let response = () => Promise.resolve({ ...model(b.toString()), dependency_candidates: [b.toString()] });
  client.client.sendRequest = () => response();
  try {
    await client.modelInfo(b); response = () => pending.promise;
    const proof = client.modelInfo(b, b, true); await flush();
    host.documents[0].version = 2;
    host.edit.fire({ document: host.documents[0], contentChanges: [{ text: "var y;" }] });
    pending.resolve({ ...model(b.toString()), dependency_candidates: [b.toString()] });
    assert.equal((await proof).root_uri, b.toString());
  } finally { await client.shutdown(); }
});

test("closed model snapshots retain recent results within the idle cache bound", async () => {
  // Guards persistent model information retaining every previously visited closed model.
  reset(); const client = service(); await client.ensureStarted();
  const roots = Array.from({ length: 12 }, (_, index) => Uri.parse(`file:///workspace/closed${index}.mod`)), calls = [];
  client.client.sendRequest = (_method, params) => {
    const root = params.arguments[0].root_uri; calls.push(root);
    return Promise.resolve({ ...model(root), document_version: null, dependency_candidates: [root] });
  };
  try {
    for (const root of roots) await client.modelInfo(root);
    await client.modelInfo(roots.at(-1));
    assert.equal(calls.length, roots.length, "a recent closed model reuses its result");
    await client.modelInfo(roots[0]);
    assert.equal(calls.length, roots.length + 1, "the oldest idle result was evicted");
  } finally { await client.shutdown(); }
});

test("shared include edits invalidate all proven owners but retain independent roots", async () => {
  // Guards stale shared-include facts when cache invalidation becomes root-scoped.
  reset(); const client = service(); await client.ensureStarted();
  const roots = ["a", "b", "c"].map(name => Uri.parse(`file:///workspace/${name}.mod`));
  const include = Uri.parse("file:///workspace/shared.inc"), calls = [];
  host.documents.push(...roots.map(uri => ({ uri, languageId: "dynare", version: 1 })));
  client.client.sendRequest = (_method, params) => {
    const root = params.arguments[0].root_uri; calls.push(root);
    return Promise.resolve({ ...model(root), dependency_candidates: [root, ...(root === roots[2].toString() ? [] : [include.toString()])] });
  };
  try {
    await Promise.all(roots.map(root => client.modelInfo(root)));
    host.edit.fire({ document: { uri: include, languageId: "dynare", version: 2 }, contentChanges: [{ text: "var y;" }] });
    await Promise.all(roots.map(root => client.modelInfo(root)));
    assert.deepEqual(roots.map(root => calls.filter(value => value === root.toString()).length), [2, 2, 1]);
  } finally { await client.shutdown(); }
});

test("macro requests and native providers share activity that restart, server stop and shutdown clear", async context => {
  // Guards activity missing direct executeCommand calls or surviving a retired server instance.
  context.mock.timers.enable({ apis: ["setTimeout"] });
  reset();
  const root = Uri.parse("file:///workspace/root.mod"), request = { textDocument: { uri: root.toString() } };
  const expansion = deferred(), hover = deferred(), final = deferred();
  const client = service({ request: (method, params) => params?.command === "dynare/showEffectiveModel" ? expansion.promise
    : method === "textDocument/hover" ? hover.promise : final.promise });
  try {
    await client.ensureStarted();
    const old = client.execute("dynare/showEffectiveModel", [root.toString()]);
    await flush(); context.mock.timers.tick(250);
    assert.equal(client.modelActivity(root).label, "Expanding macros");
    await client.restart(); assert.equal(client.modelActivity(root), undefined);
    const current = client.client.sendRequest("textDocument/hover", request);
    context.mock.timers.tick(250); assert.equal(client.modelActivity(root).label, "Loading hover");
    expansion.resolve({}); await old;
    assert.equal(client.modelActivity(root).label, "Loading hover");
    client.client.state({ newState: languageClient.State.Stopped });
    assert.equal(client.modelActivity(root), undefined);
    hover.resolve(); await current;
    await client.restart();
    const last = client.client.sendRequest("textDocument/references", request);
    context.mock.timers.tick(250); assert.equal(client.modelActivity(root).label, "Finding references");
    await client.shutdown();
    assert.equal(client.modelActivity(root), undefined);
    final.resolve(); await last;
  } finally { await client.shutdown(); }
});

test("restart closes the protocol before destroying the server input stream", async () => {
  // Guards Restart Language Server producing ERR_STREAM_DESTROYED or EPIPE on shutdown writes.
  reset();
  const childProcess = require("node:child_process"), { PassThrough } = require("node:stream");
  const previousSpawn = childProcess.spawn, previousClient = languageClient.LanguageClient;
  const errors = [], children = [];
  childProcess.spawn = () => {
    const child = { stdin: new PassThrough(), stderr: new PassThrough(), stdout: new PassThrough(), exitCode: null, signalCode: null, killed: false,
      kill() { this.killed = true; this.stdin.destroy(); return true; } };
    child.stdin.on("error", () => {}); children.push(child); return child;
  };
  languageClient.LanguageClient = class {
    initializeResult = { capabilities: {}, serverInfo: { name: "dygnosis", version: "test" } };
    state = 3;
    constructor(_id, _name, server) { this.server = server; }
    registerFeature() {}
    async start() { this.child = await this.server(); this.state = 2; }
    getFeature() { return { clear() {} }; }
    sendNotification() { return Promise.resolve(); }
    onNotification() { return new Disposable(); }
    onDidChangeState() { return new Disposable(); }
    handleFailedRequest(_type, _token, error) { throw error; }
    dispose() {
      return new Promise(resolve => this.child.stdin.write("shutdown", error => {
        if (error) errors.push(error.code);
        else { this.child.exitCode = 0; this.child.stdin.end(); }
        resolve();
      }));
    }
  };
  const client = new DygnosisClient({}, { resolve: () => Promise.resolve({ path: "engine", override: true, version: "test" }) });
  try {
    await client.ensureStarted(); const previous = client.client; await client.restart();
    const failure = new Error("Starting server failed");
    assert.equal(previous.handleFailedRequest({ method: "textDocument/documentLink" }, undefined, failure, null), null);
    assert.throws(() => client.client.handleFailedRequest({ method: "textDocument/documentLink" }, undefined, failure, null), error => error === failure);
    await client.shutdown();
    assert.deepEqual(errors, []);
    assert.equal(children.length, 2);
    assert.ok(children.every(child => child.exitCode === 0));
  } finally {
    await client.shutdown(); childProcess.spawn = previousSpawn; languageClient.LanguageClient = previousClient;
    for (const child of children) { child.stdin.destroy(); child.stdout.destroy(); child.stderr.destroy(); }
  }
});

function analysisContext(root, generation = 0, name = null) {
  return {schema_version:1,root_uri:root.toString(),document_uri:root.toString(),document_version:null,revision:`source-${generation}`,
    analysis:{state:"selected",selection:{configuration_id:`choice-${generation}`,name,preview:false,invocation_scope:"explicit_macro_inputs"},
      configuration:name ? {name} : {},selection_generation:generation,include_paths:null,macro_complete:true,model_complete:true,overrides:[]},
    sites:[],source_coverage:{complete:true}};
}

test("a delayed analysis acknowledgment cannot relabel a newer accepted configuration", async () => {
  // The server can accept two ordered choices before the first reply reaches a view.
  reset(); const client = service({analysis:true}); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), delayed = deferred(), events = [];
  let current = analysisContext(root);
  client.execute = async (command, args) => {
    if (command === "dynare/analysisContext") return globalThis.structuredClone(current);
    if (args[0].analysis.name === "First") { current = analysisContext(root,1,"First"); return delayed.promise; }
    current = analysisContext(root,2,"Second"); return {...current,accepted:true};
  };
  const subscription = client.onDidChangeAnalysis(event => events.push(event.context.analysis.selection.name));
  const firstProof = await client.analysisContext(root);
  const first = client.setAnalysisConfiguration(root,{name:"First"},firstProof,null); void first.catch(() => {});
  await flush();
  const secondProof = await client.analysisContext(root);
  await client.setAnalysisConfiguration(root,{name:"Second"},secondProof,null);
  delayed.resolve({...analysisContext(root,1,"First"),accepted:true});
  await assert.rejects(first,/newer accepted analysis configuration/);
  assert.deepEqual(events,["Second"]);
  subscription.dispose(); await client.shutdown();
});

test("accepted partial analysis updates selection before refusing its dependent action", async () => {
  // A failed follow-up source inspection must not leave the UI on the old choice.
  reset(); const client = service({analysis:true}); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), events = [];
  client.execute = async command => command === "dynare/analysisContext" ? analysisContext(root) :
    {...analysisContext(root,1,"Accepted"),accepted:true,revision:"",partial:true,message:"Accepted, but source inspection reached its allowance."};
  const subscription = client.onDidChangeAnalysis(event => events.push(event.context.analysis.selection.name));
  const proof = await client.analysisContext(root);
  await assert.rejects(client.setAnalysisConfiguration(root,{name:"Accepted"},proof,null),/Accepted, but source inspection/);
  assert.deepEqual(events,["Accepted"]);
  await client.restart();
  await assert.rejects(client.setAnalysisConfiguration(root,{},proof,null),/selector is no longer current/);
  subscription.dispose(); await client.shutdown();
});

test("an obsolete startup cannot clear the replacement startup promise", async () => {
  // Guards a late resolver allowing background requests to start a third replacement.
  reset();
  const old = deferred(), replacement = deferred(); let resolutions = 0;
  const client = service({ resolve: () => ++resolutions === 1 ? old.promise : replacement.promise });
  const first = client.ensureStarted(); await flush();
  const restarted = client.restart(); await flush();
  let joined;
  try {
    old.resolve({ path: "old", override: true, version: "old" }); await first;
    joined = client.ensureStarted(); await flush();
    assert.equal(resolutions, 2, "the request must join the pending replacement");
  } finally {
    replacement.resolve({ path: "new", override: true, version: "new" });
    await Promise.all([first, restarted, joined]); await client.shutdown();
  }
});

test("native Rename application has one command owner and no static or dynamic provider bypass", async () => {
  reset(); const client = service(); await client.ensureStarted();
  assert.ok(host.commands.has("dygnosis.renameSymbol"));
  assert.ok(host.clearedFeatures.includes("textDocument/rename"));
  let registered;
  await client.middleware.handleRegisterCapability({ registrations: [{ id: "unsafe", method: "textDocument/rename" }, { id: "kept", method: "textDocument/hover" }] }, async params => { registered = params.registrations; });
  assert.deepEqual(registered, [{ id: "kept", method: "textDocument/hover" }]);
  await client.shutdown();
});
test("startup sends latest settings and reconciles folders changed while initialize was pending", async () => {
  reset(); const client = service({ pending: true }); const starting = client.ensureStarted();
  await flush();
  host.settings.searchPaths = ["latest"];
  host.folders.push({ uri: Uri.parse("file:///added"), name: "added" });
  host.folder.fire({ added: [host.folders[1]], removed: [] });
  host.managed[0].initialized.resolve(); await starting;
  const folder = host.notifications.find(row => row.method === "workspace/didChangeWorkspaceFolders");
  assert.equal(folder.params.event.added[0].uri, "file:///added");
  const settings = host.notifications.findLast(row => row.method === "workspace/didChangeConfiguration").params.settings;
  assert.deepEqual(settings.dynare.configuration.loose.searchPaths, ["latest"]);
  assert.equal(settings.dynare.configuration.folders.length, 2);
  assert.equal(client.supportsModelInfo, true); await client.shutdown();
});

test("macro tint alone sends no server settings or input invalidation", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const events = [], listener = client.onDidInvalidate(event => events.push(event));
  const count = host.notifications.length;
  host.config.fire({ affectsConfiguration: key => ["dynare", "dynare.effectiveModel.macroTint"].includes(key) });
  await flush();
  assert.equal(events.length, 0);
  assert.equal(host.notifications.length, count);
  host.config.fire({ affectsConfiguration: key => ["dynare", "dynare.effectiveModel.macroTint", "dynare.searchPaths"].includes(key) });
  await flush();
  assert.equal(events.length, 1);
  assert.ok(host.notifications.length > count);
  listener.dispose(); await client.shutdown();
});

test("server invalidation keeps the diagnostic input token separate from the model revision", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const events = [], listener = client.onDidInvalidate(event => events.push(event));
  host.managed[0].client.notify({ schema_version: 1, root_uri: "file:///workspace/root.mod", revision: "model-token", input_revision: "diagnostic-token" });
  assert.equal(events[0].inputRevision, "diagnostic-token");
  assert.equal(events[0].modelRevision, "model-token");
  assert.equal(events[0].root, "file:///workspace/root.mod");
  listener.dispose(); await client.shutdown();
});
test("edits and disk/server invalidations coalesce a native Outline refresh", async () => {
  reset(); const client = service(); await client.ensureStarted(); const before = host.symbols;
  const included = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 2 };
  host.edit.fire({ document: included, contentChanges: [{ text: "changed" }] }); host.disk.fire(included.uri);
  host.managed[0].client.notify({ schema_version: 1, root_uri: "file:///workspace/root.mod" });
  await new Promise(resolve => setTimeout(resolve, 110));
  assert.equal(host.symbols, before + 1); await client.shutdown();
});
test("unavailable owner responses preserve a selected owner until current facts disprove it", async () => {
  reset(); const client = service(); const root = Uri.parse("file:///workspace/root.mod"), child = { uri: Uri.parse("file:///workspace/fragment.mod"), languageId: "dynare", version: 1 };
  client.selectOwner(child.uri, root);
  client.modelInfo = () => Promise.resolve(undefined);
  assert.equal(await client.rootForDocument(child), undefined);
  client.modelInfo = () => Promise.resolve(model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: child.uri.fsPath }]));
  assert.equal((await client.rootForDocument(child)).toString(), root.toString());
  client.modelInfo = () => Promise.resolve(model());
  assert.equal((await client.rootForDocument(child)).toString(), child.uri.toString(), "removed edge restores the ordinary .mod fallback");
  client.treatAsRoot(child.uri);
  assert.equal((await client.rootForDocument(child)).toString(), child.uri.toString()); await client.shutdown();
});

// Guards suffix-first standalone selection and a cached automatic owner becoming
// an arbitrary choice when another verified parent is discovered.
test("mod helpers discover owners before standalone and automatic choices yield to ambiguity", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod");
  const helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, helper);
  const related = [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }];
  host.managed[0].client.sendRequest = (_method, params) => Promise.resolve(model(params.arguments[0].root_uri, related));
  try {
    await client.modelInfo(helper.uri); assert.deepEqual(client.knownOwners(helper.uri), [], "a model is not its own parent");
    assert.equal((await client.rootForDocument(helper)).toString(), a.toString());
    assert.equal(client.chosenRootForDocument(helper), undefined, "automatic ownership is not explicit workspace priority");
    host.documents.push({ uri: b, languageId: "dynare", version: 1 }); await client.modelInfo(b);
    assert.equal(await client.rootForDocument(helper), undefined, "old automatic owner must not hide a second parent");
    assert.equal(host.picks.length, 0);
    client.treatAsRoot(helper.uri);
    assert.equal((await client.rootForDocument(helper)).toString(), helper.uri.toString(), "explicit standalone wins over both parents");
    helper.isClosed = true; host.close.fire(helper);
    helper.isClosed = false; host.open.fire(helper); await client.modelInfo(a); await client.modelInfo(b);
    assert.equal(await client.rootForDocument(helper), undefined, "reopen clears standalone intent");
  } finally { await client.shutdown(); }
});

// Guards local/server context divergence: publish the native choice only after
// the server accepts the exact document version and model revision.
test("guarded owner and standalone choices reach the server before changing the native view", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 7 };
  host.documents.push({ uri: root, languageId: "dynare", version: 1 }, helper);
  const related = [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }];
  const calls = [], accepted = deferred();
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0]; calls.push(params);
    return params.command === "dynare/setDocumentContext" ? accepted.promise : Promise.resolve({ ...model(arg.root_uri, related), document_uri: arg.document_uri, document_version: arg.document_uri === helper.uri.toString() ? helper.version : 1 });
  };
  try {
    await client.modelInfo(root);
    const choosing = client.applyOwner(helper, root); await flush();
    assert.equal(client.chosenRootForDocument(helper), undefined);
    assert.deepEqual(calls.find(call => call.command === "dynare/setDocumentContext").arguments, [{ document_uri: helper.uri.toString(), document_version: 7, root_uri: root.toString(), expected_revision: "one" }]);
    accepted.resolve({ schema_version: 1 }); await choosing;
    assert.equal(client.chosenRootForDocument(helper).toString(), root.toString());
    await client.treatDocumentAsRoot(helper);
    assert.equal(calls.at(-1).arguments[0].root_uri, helper.uri.toString());
    assert.equal((await client.rootForDocument(helper)).toString(), helper.uri.toString());
  } finally { await client.shutdown(); }
});

test("refused and stale owner choices retain the current native context", async () => {
  for (const outcome of ["refused", "edited", "restarted"]) {
    reset(); const client = service({ documentContext: true }); await client.ensureStarted();
    const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
    host.documents.push(helper); client.treatAsRoot(helper.uri);
    const proof = deferred(), calls = [];
    host.managed[0].client.sendRequest = (_method, params) => {
      calls.push(params.command);
      return params.command === "dynare/modelInfo" ? proof.promise : Promise.resolve({ error: "The model inputs changed. Choose its model context again." });
    };
    const choosing = client.applyOwner(helper, root); await flush();
    if (outcome === "edited") helper.version = 2;
    if (outcome === "restarted") await client.restart();
    proof.resolve({ ...model(root.toString(), [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: helper.uri.toString(), document_version: 1 });
    await assert.rejects(choosing, /changed/);
    if (outcome !== "refused") assert.ok(!calls.includes("dynare/setDocumentContext"), "stale proof cannot send a context mutation");
    if (outcome !== "restarted") assert.equal((await client.rootForDocument(helper)).toString(), helper.uri.toString());
    await client.shutdown();
  }
});
test("older engines keep the useful model choice and explain the unsynchronized diagnostics at the command", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper);
  try {
    const before = host.notifications.length;
    await client.treatDocumentAsRoot(helper);
    assert.equal((await client.rootForDocument(helper)).toString(), helper.uri.toString());
    const messages = host.notifications.slice(before).filter(value => typeof value === "string");
    assert.equal(messages.length, 1);
    assert.match(messages[0], /model information and macro preview/);
    assert.match(messages[0], /diagnostics.*automatic context.*selected engine cannot accept owner choices/);
    assert.match(messages[0], /bundled binary.*dynare.serverPath/);
  } finally { await client.shutdown(); }
});
test("closing the last text tab resets a retained document's native and server context", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper); const mutations = [];
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") { mutations.push(arg); return Promise.resolve({ schema_version: 1 }); }
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  try {
    await client.applyOwner(helper, root);
    const tab = { input: new TabInputText(helper.uri) }; host.tabGroups = [];
    host.tabsChanged.fire({ closed: [tab], opened: [], changed: [] }); await flush();
    assert.equal(helper.isClosed, undefined, "VS Code can retain the TextDocument after its last tab closes");
    assert.equal(client.chosenRootForDocument(helper), undefined);
    assert.deepEqual(mutations.at(-1), { document_uri: helper.uri.toString(), document_version: 1, root_uri: null });
  } finally { await client.shutdown(); }
});
test("another text or diff view retains context until the last matching tab closes", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper); const mutations = [];
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") { mutations.push(arg.root_uri); return Promise.resolve({ schema_version: 1 }); }
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  try {
    await client.applyOwner(helper, root);
    const textTab = { input: new TabInputText(helper.uri) }, diffTab = { input: new TabInputTextDiff(Uri.parse("file:///workspace/before.mod"), helper.uri) };
    host.tabGroups = [{ tabs: [diffTab] }]; host.tabsChanged.fire({ closed: [textTab], opened: [], changed: [] }); await flush();
    assert.equal(client.chosenRootForDocument(helper).toString(), root.toString()); assert.deepEqual(mutations, [root.toString()]);
    host.tabGroups = []; host.tabsChanged.fire({ closed: [diffTab], opened: [], changed: [] }); await flush();
    assert.equal(client.chosenRootForDocument(helper), undefined); assert.deepEqual(mutations, [root.toString(), null]);
  } finally { await client.shutdown(); }
});
test("last-tab closure clears an accepted owner and prevents its delayed acknowledgment from restoring it", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper); const acknowledgment = deferred(); let serverOwner;
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") { serverOwner = arg.root_uri; return arg.root_uri === null ? Promise.resolve({ schema_version: 1 }) : acknowledgment.promise; }
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  try {
    const pending = client.applyOwner(helper, root); await flush(); assert.equal(serverOwner, root.toString());
    host.tabGroups = []; host.tabsChanged.fire({ closed: [{ input: new TabInputText(helper.uri) }], opened: [], changed: [] }); await flush();
    acknowledgment.resolve({ schema_version: 1 }); await assert.rejects(pending, /replaced this choice/);
    assert.equal(serverOwner, null); assert.equal(client.chosenRootForDocument(helper), undefined);
  } finally { await client.shutdown(); }
});
test("a background tab-reset refusal logs the cleared choice and actual cause without a popup", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper); const logs = []; client.log = message => logs.push(message);
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") return Promise.resolve(arg.root_uri === null ? { error: "The displayed document version changed." } : { schema_version: 1 });
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  try {
    await client.applyOwner(helper, root);
    const before = host.notifications.filter(value => typeof value === "string").length;
    host.tabsChanged.fire({ closed: [{ input: new TabInputText(helper.uri) }], opened: [], changed: [] }); await flush();
    assert.equal(client.chosenRootForDocument(helper), undefined);
    assert.ok(logs.some(message => /choice was cleared.*diagnostics could not return to automatic context.*displayed document version changed/.test(message)));
    assert.equal(host.notifications.filter(value => typeof value === "string").length, before);
  } finally { await client.shutdown(); }
});
test("a delayed accepted context keeps client and server aligned when the source-jump guard expires", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: root, languageId: "dynare", version: 1 }, helper);
  host.editor = { document: host.documents[0] }; client.treatAsRoot(helper.uri);
  const acknowledgment = deferred(); let serverOwner = helper.uri.toString(), valid = true;
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") { serverOwner = arg.root_uri; return acknowledgment.promise; }
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  const originalOpen = vscode.workspace.openTextDocument; vscode.workspace.openTextDocument = async () => helper;
  try {
    const pending = client.openLocation({ uri: helper.uri.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } }, root, () => ({ isCurrent: () => valid }));
    await flush(); assert.equal(serverOwner, root.toString(), "the server already accepted the choice");
    valid = false; acknowledgment.resolve({ schema_version: 1 });
    await assert.rejects(pending, /model choice was applied.*pending action stopped.*source or model changed/);
    assert.equal(client.chosenRootForDocument(helper).toString(), serverOwner);
    assert.equal(host.reveals.length, 0, "the expired source jump is withheld");
  } finally { vscode.workspace.openTextDocument = originalOpen; await client.shutdown(); }
});
test("an older delayed context acknowledgment cannot overwrite a newer accepted owner", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper); const firstAck = deferred(); let serverOwner;
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") { serverOwner = arg.root_uri; return arg.root_uri === a.toString() ? firstAck.promise : Promise.resolve({ schema_version: 1 }); }
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  try {
    const first = client.applyOwner(helper, a); await flush();
    await client.applyOwner(helper, b); firstAck.resolve({ schema_version: 1 });
    await assert.rejects(first, /newer model choice/);
    assert.equal(serverOwner, b.toString()); assert.equal(client.chosenRootForDocument(helper).toString(), serverOwner);
  } finally { await client.shutdown(); }
});
test("an accepted owner acknowledgment survives a newer owner attempt whose proof fails", async () => {
  reset(); const client = service({ documentContext: true }); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), helper = { uri: Uri.parse("file:///workspace/helper.mod"), languageId: "dynare", version: 1 };
  host.documents.push(helper); client.treatAsRoot(helper.uri);
  const firstAck = deferred(), secondProof = deferred(), mutations = []; let serverOwner = helper.uri.toString();
  host.managed[0].client.sendRequest = (_method, params) => {
    const arg = params.arguments[0];
    if (params.command === "dynare/setDocumentContext") { mutations.push(arg.root_uri); serverOwner = arg.root_uri; return firstAck.promise; }
    if (arg.root_uri === b.toString()) return secondProof.promise;
    return Promise.resolve({ ...model(arg.root_uri, [{ kind: "include", filename: "helper.mod", resolved: true, path: helper.uri.fsPath }]), document_uri: arg.document_uri });
  };
  try {
    const first = client.applyOwner(helper, a); await flush();
    assert.equal(serverOwner, a.toString(), "A was accepted before its delayed acknowledgment");
    const second = client.applyOwner(helper, b); await flush();
    firstAck.resolve({ schema_version: 1 });
    await assert.rejects(first, /model choice was applied.*another model choice was requested/);
    assert.equal(client.chosenRootForDocument(helper).toString(), a.toString(), "the pending B attempt cannot displace accepted A");
    secondProof.resolve({ code: "ROOT_NOT_FOUND", error: "The model root is unavailable" });
    await assert.rejects(second, /root is unavailable/);
    assert.deepEqual(mutations, [a.toString()], "failed B never changes the server context");
    assert.equal(client.chosenRootForDocument(helper).toString(), serverOwner);
  } finally { await client.shutdown(); }
});
test("an older-engine interactive owner choice explains the partial outcome once while automatic context stays quiet", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), helper = { uri: Uri.parse("file:///workspace/helper.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, { uri: b, languageId: "dynare", version: 1 }, helper);
  host.managed[0].client.sendRequest = (_method, params) => Promise.resolve(model(params.arguments[0].root_uri, [{ kind: "include", filename: "helper.inc", resolved: true, path: helper.uri.fsPath }]));
  try {
    await client.modelInfo(a); await client.modelInfo(b);
    const before = host.notifications.filter(value => typeof value === "string").length;
    assert.equal(await client.rootForDocument(helper, false), undefined);
    assert.equal(host.notifications.filter(value => typeof value === "string").length, before);
    assert.equal((await client.rootForDocument(helper, true)).toString(), a.toString());
    const messages = host.notifications.filter(value => typeof value === "string").slice(before);
    assert.equal(messages.length, 1); assert.match(messages[0], /model information and macro preview.*diagnostics.*cannot accept owner choices/);
    await client.rootForDocument(helper, false);
    assert.equal(host.notifications.filter(value => typeof value === "string").length, before + 1);
  } finally { await client.shutdown(); }
});
test("a newer selected owner survives both an old owner reply and an old ownership disproof", async () => {
  for (const stillOwned of [false, true]) {
    reset(); const client = service(), pending = deferred();
    const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod");
    const child = { uri: Uri.parse("file:///workspace/fragment.mod"), languageId: "dynare", version: 1 };
    const related = [{ kind: "include", filename: "fragment.mod", resolved: true, path: child.uri.fsPath }];
    client.modelInfo = root => root.toString() === a.toString() ? pending.promise : Promise.resolve(model(b.toString(), related));
    client.selectOwner(child.uri, a); const obsolete = client.rootForDocument(child);
    client.selectOwner(child.uri, b); pending.resolve(model(a.toString(), stillOwned ? related : []));
    assert.equal(await obsolete, undefined);
    assert.equal((await client.rootForDocument(child)).toString(), b.toString());
    await client.shutdown();
  }
});
test("model snapshots canceled by a delivered engine invalidation can be retried with current facts", async () => {
  reset(); const client = service(); await client.ensureStarted(); const pending = deferred();
  const root = Uri.parse("file:///workspace/root.mod");
  host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  host.managed[0].client.sendRequest = () => pending.promise;
  const obsolete = client.modelInfo(root); await flush();
  host.managed[0].client.notify({ schema_version: 1, root_uri: root.toString(), revision: "one" });
  pending.resolve(model()); assert.equal(await obsolete, undefined);
  host.managed[0].client.sendRequest = () => Promise.resolve(model());
  const current = await client.modelInfo(root);
  assert.equal(current.revision, "one"); assert.equal(current.client_instance, client.currentInstance);
  await client.shutdown();
});
test("an invalid replacement resolver disposes the old process instead of retaining its engine", async () => {
  reset(); let fail = false;
  const client = service({ resolve: () => fail ? Promise.reject(new Error("invalid override")) : Promise.resolve({ path: "engine", version: "one", override: true }) });
  await client.ensureStarted(); const old = host.managed[0]; fail = true;
  await assert.rejects(client.restart(), /could not start.*invalid override/); assert.equal(old.disposed, true); assert.equal(client.client, undefined);
  // A failed replacement must release its startup promise so a corrected path can retry.
  fail = false; await client.ensureStarted(); assert.ok(client.client); assert.equal(host.managed.length, 2); await client.shutdown();
});

// Guards an automatic restart rejecting outside a command boundary.
test("a failed settings restart is logged once and can retry without a notification", async () => {
  reset(); let fail = false;
  const client = service({ resolve: async () => { if (fail) throw new Error("invalid override"); return { path: "engine", version: "one", override: true }; } });
  await client.ensureStarted(); const notices = host.notifications.length, logs = []; client.log = value => logs.push(value); fail = true;
  host.config.fire({ affectsConfiguration: name => name === "dynare" || name === "dynare.serverPath" }); await flush();
  assert.equal(logs.length, 1); assert.match(logs[0], /could not start.*invalid override/); assert.equal(host.notifications.length, notices);
  fail = false; await client.ensureStarted(); assert.ok(client.client); await client.shutdown();
});

// Guards a cold-start failure showing both the helper error and a command refusal.
test("an explicit command shows one notification for the original startup cause", async () => {
  reset();
  const client = service({ resolve: async () => { throw new Error("invalid override"); } });
  await runCommand(client, () => client.execute("dynare/modelInfo", []));
  assert.equal(host.notifications.length, 1);
  assert.match(host.notifications[0], /could not start.*invalid override/);
  await client.shutdown();
});

// Guards explicit navigation losing the known cause when an engine lacks supported ModelInfo.
test("an unsupported model-info capability reaches the command notification", async () => {
  reset(); const client = service({ pending: true }), starting = client.ensureStarted(); await flush();
  host.managed[0].client.initializeResult.capabilities.experimental.dygnosis.modelInfo.schema_version = 99;
  host.managed[0].initialized.resolve(); await starting;
  await runCommand(client, () => client.modelInfo(Uri.parse("file:///workspace/root.mod")));
  const notices = host.notifications.filter(value => typeof value === "string");
  assert.equal(notices.length, 1); assert.match(notices[0], /selected engine does not support model information.*bundled binary/);
  await client.shutdown();
});
test("late old-instance notifications cannot invalidate the replacement client's Outline", async () => {
  reset(); const client = service(); await client.ensureStarted(); const old = host.managed[0].client;
  await client.restart(); const before = host.symbols;
  let changed = 0; const listener = client.onDidChange(() => { ++changed; });
  old.notify({ schema_version: 1, root_uri: "file:///old.mod" }); old.state({ newState: 1 });
  assert.equal(changed, 0); listener.dispose();
  await new Promise(resolve => setTimeout(resolve, 110));
  assert.equal(client.supportsModelInfo, true); assert.equal(host.symbols, before + 1, "only the replacement's own pending refresh runs");
  await client.shutdown();
});
test("a slow obsolete resolver cannot replace a newer restarted client", async () => {
  reset(); const slow = deferred(); let call = 0;
  const client = service({ resolve: () => ++call === 1 ? slow.promise : Promise.resolve({ path: "new engine", override: true, version: "two" }) });
  const first = client.ensureStarted(); await flush();
  await client.restart(); const current = client.client;
  slow.resolve({ path: "old engine", override: true, version: "one" }); await first;
  assert.equal(client.client, current); assert.equal(host.managed.length, 1); await client.shutdown();
});
test("restart invalidates the old engine instance before the replacement resolver can finish", async () => {
  reset(); const pending = deferred(); let resolutions = 0;
  const client = service({ resolve: () => ++resolutions === 1 ? Promise.resolve({ path: "engine", override: true, version: "one" }) : pending.promise });
  await client.ensureStarted(); const instance = client.currentInstance, events = [], listener = client.onDidInvalidate(event => events.push(event));
  const restarted = client.restart(); await flush();
  assert.ok(client.currentInstance > instance); assert.ok(events.some(event => event.reason === "lifecycle")); assert.equal(host.managed.length, 1, "replacement engine remains unresolved");
  pending.resolve({ path: "replacement", override: true, version: "two" }); await restarted; listener.dispose(); await client.shutdown();
});
test("engine stop and explicit shutdown invalidate their instance immediately", async () => {
  for (const operation of ["stop", "shutdown"]) {
    reset(); const client = service(); await client.ensureStarted(); const instance = client.currentInstance, events = [], listener = client.onDidInvalidate(event => events.push(event));
    if (operation === "stop") host.managed[0].client.state({ newState: languageClient.State.Stopped }); else await client.shutdown();
    assert.ok(client.currentInstance > instance, operation); assert.ok(events.some(event => event.reason === "lifecycle"), operation);
    listener.dispose(); if (operation === "stop") await client.shutdown();
  }
});
test("native include links route through the retained owner before opening a mod fragment", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/fragment.mod");
  host.links = () => [{ target: target.toString() }];
  const source = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(source);
  const info = { ...model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }]), client_instance: client.currentInstance };
  client.modelInfo = () => Promise.resolve(info);
  const links = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, target)]);
  assert.equal(links[0].target.scheme, "command");
  const [key] = JSON.parse(decodeURIComponent(links[0].target.query));
  await host.commands.get("dygnosis.openIncludeLink")(key);
  assert.deepEqual(host.opened, [target.toString()]);
  assert.equal((await client.rootForDocument(host.documents.at(-1))).toString(), root.toString()); await client.shutdown();
});
test("an editor-cached native link remains usable after another same-version provider request", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/fragment.mod");
  host.links = () => [{ target: target.toString() }];
  const source = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(source); host.editor = { document: source };
  const info = { ...model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }]), client_instance: client.currentInstance };
  client.modelInfo = () => Promise.resolve(info);
  const provider = () => [new vscode.DocumentLink({}, target)];
  const first = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, provider);
  const second = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, provider);
  assert.equal(second[0].target.toString(), first[0].target.toString(), "repeated provider calls reuse one durable command");
  const [cachedKey] = JSON.parse(decodeURIComponent(first[0].target.query));
  await host.commands.get("dygnosis.openIncludeLink")(cachedKey);
  assert.deepEqual(host.opened, [target.toString()], "the native editor may retain the first provider response");
  await client.shutdown();
});
test("native link commands are retired after source-version changes, close, replacement and restart", async () => {
  for (const changed of ["version", "close", "target", "restart"]) {
    reset(); const client = service(); await client.ensureStarted();
    const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/fragment.mod");
    const source = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(source); host.editor = { document: source };
    client.modelInfo = () => Promise.resolve({ ...model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }]), client_instance: client.currentInstance });
    const first = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, target)]);
    const [key] = JSON.parse(decodeURIComponent(first[0].target.query));
    if (changed === "version") { source.version = 2; host.edit.fire({ document: source, contentChanges: [{ text: "changed" }] }); }
    else if (changed === "close") host.close.fire(source);
    else if (changed === "target") await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, Uri.parse("file:///workspace/replacement.mod"))]);
    else await client.restart();
    await host.commands.get("dygnosis.openIncludeLink")(key);
    assert.deepEqual(host.opened, [], `${changed} must invalidate the saved command`);
    await client.shutdown();
  }
});
test("native include validation refuses a newer owner or editor selected while its facts are pending", async () => {
  for (const changed of ["owner", "editor"]) {
    reset(); const client = service(); await client.ensureStarted(); const pending = deferred();
    const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), target = Uri.parse("file:///workspace/fragment.mod");
    host.links = () => pending.promise.then(() => [{ target: target.toString() }]);
    const source = { uri: a, languageId: "dynare", version: 1 }; host.documents.push(source); host.editor = { document: source };
    const related = [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }];
    const info = { ...model(a.toString(), related), client_instance: client.currentInstance };
    client.modelInfo = () => pending.promise;
    const links = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, target)]);
    const [key] = JSON.parse(decodeURIComponent(links[0].target.query));
    const opening = host.commands.get("dygnosis.openIncludeLink")(key); await flush();
    if (changed === "owner") client.selectOwner(source.uri, b);
    else host.editor = { document: { uri: b, languageId: "dynare", version: 1 } };
    pending.resolve(info); await opening;
    assert.deepEqual(host.opened, []);
    client.modelInfo = () => Promise.resolve(model(b.toString(), [{ kind: "include", filename: "a.mod", resolved: true, path: a.fsPath }]));
    if (changed === "owner") assert.equal((await client.rootForDocument(source)).toString(), b.toString());
    await client.shutdown();
  }
});
test("native include opening accepts an intentional owner pick before guarding validation", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/fragment.mod");
  host.links = () => [{ target: target.toString() }];
  const source = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push(source); host.editor = { document: source };
  const info = { ...model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }]), client_instance: client.currentInstance };
  client.rootForDocument = document => { if (document === source) client.selectOwner(source.uri, root); return Promise.resolve(root); };
  client.modelInfo = () => Promise.resolve(info);
  const links = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, target)]);
  const [key] = JSON.parse(decodeURIComponent(links[0].target.query));
  await host.commands.get("dygnosis.openIncludeLink")(key);
  assert.deepEqual(host.opened, [target.toString()]); await client.shutdown();
});
test("navigation checks a caller guard after loading and before ownership or editor changes", async () => {
  reset(); const client = service(), pending = deferred();
  const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/fragment.mod");
  const loaded = { uri: target, languageId: "dynare", version: 1 };
  const originalOpen = vscode.workspace.openTextDocument;
  vscode.workspace.openTextDocument = () => pending.promise;
  let current = true, sawLoaded = false;
  const location = { uri: target.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
  const jump = client.openLocation(location, root, document => { sawLoaded = document === loaded; return current; });
  current = false; pending.resolve(loaded); await assert.rejects(jump, /source changed/);
  assert.equal(sawLoaded, true); assert.deepEqual(host.opened, []);
  assert.equal((await client.rootForDocument(loaded)).toString(), target.toString(), "rejected navigation must not assign an owner");
  vscode.workspace.openTextDocument = originalOpen;
  await client.openLocation(location, root, document => Promise.resolve(document.uri.toString() === target.toString()));
  assert.deepEqual(host.opened, [target.toString()], "loading a current unopened source is allowed");
  client.modelInfo = () => Promise.resolve(model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }]));
  assert.equal((await client.rootForDocument(host.documents.at(-1))).toString(), root.toString());
  await client.shutdown();
});

test("navigation capabilities advertise the agreed comparison and effective-preview schemas", () => {
  const capabilities = { experimental: { retained: true } };
  extendCapabilities(capabilities);
  assert.equal(capabilities.experimental.retained, true);
  assert.deepEqual(capabilities.experimental.dygnosis.compareModels, { navigation_schema_version: 1 });
  assert.deepEqual(capabilities.experimental.dygnosis.effectivePreview, { navigation_schema_version: 1, dependency_candidates: true });
  assert.equal(capabilities.experimental.dygnosis.modelInfo.schema_version, 1);
  assert.equal(capabilities.experimental.dygnosis.workspaceStatusChanged, true);
});
test("chosen context reads only a proven selected owner and restores it on a separate model-info event", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod");
  const child = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, { uri: b, languageId: "dynare", version: 1 }, child);
  let requests = 0;
  const related = [{ kind: "include", filename: "shared.inc", resolved: true, path: child.uri.fsPath }];
  host.managed[0].client.sendRequest = (_method, params) => { ++requests; return Promise.resolve(model(params.arguments[0].root_uri, related)); };
  const updates = [], choices = []; let broadChanges = 0, invalidations = 0;
  const restored = client.onDidUpdateModelInfo(info => {
    updates.push(info); choices.push(client.chosenRootForDocument(child)?.toString());
    void client.modelInfo(Uri.parse(info.root_uri));
  });
  const broad = client.onDidChange(() => { ++broadChanges; }), input = client.onDidInvalidate(() => { ++invalidations; });
  await client.modelInfo(a); await client.modelInfo(b); await flush();
  assert.equal(client.knownOwners(child.uri).length, 2); assert.equal(client.chosenRootForDocument(child), undefined, "known owners are not chosen owners");
  const beforeLookup = requests;
  client.selectOwner(child.uri, b); assert.equal(client.chosenRootForDocument(child).toString(), b.toString());
  assert.equal(requests, beforeLookup, "the synchronous accessor issues no lookup");
  client.invalidate(b.toString()); assert.equal(client.chosenRootForDocument(child), undefined, "retained selection alone is not proof");
  assert.equal(client.knownOwners(child.uri).length, 1, "remaining proof must not silently choose the other owner");
  const broadBeforeRestore = broadChanges, inputBeforeRestore = invalidations;
  await client.modelInfo(b); await flush();
  assert.equal(client.chosenRootForDocument(child).toString(), b.toString()); assert.equal(choices.at(-1), b.toString());
  assert.equal(updates.at(-1).client_instance, client.currentInstance);
  assert.equal(broadChanges, broadBeforeRestore, "restoration preserves broad navigation-change semantics");
  assert.equal(invalidations, inputBeforeRestore); assert.equal(updates.length, 3); assert.equal(requests, 3, "event readers reuse the cache without a feedback loop");
  await client.modelInfo(b); assert.equal(updates.length, 3, "cached reads do not emit proof-restoration updates");
  restored.dispose(); broad.dispose(); input.dispose(); await client.shutdown();
});
test("ambient sole-owner include presentation does not gain explicit workspace priority", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), child = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: root, languageId: "dynare", version: 1 }, child);
  const related = [{ kind: "include", filename: "shared.inc", resolved: true, path: child.uri.fsPath }];
  host.managed[0].client.sendRequest = (_method, params) => Promise.resolve({ ...model(root.toString(), related), document_uri: params.arguments[0].document_uri });
  try {
    await client.modelInfo(root); assert.equal(client.knownOwners(child.uri).length, 1); assert.equal(client.chosenRootForDocument(child), undefined);
    assert.equal((await client.rootForDocument(child, false)).toString(), root.toString(), "ambient include presentation remains convenient");
    assert.equal(client.chosenRootForDocument(child), undefined, "automatic discovery is not a user choice");
    assert.equal((await client.modelForDocument(child)).root_uri, root.toString()); assert.equal(host.picks.length, 0);
    client.invalidate(root.toString()); await client.modelInfo(root);
    assert.equal(client.chosenRootForDocument(child), undefined, "new proof does not promote automatic provenance");
    client.selectOwner(child.uri, root); assert.equal(client.chosenRootForDocument(child).toString(), root.toString(), "explicit same-owner selection upgrades the automatic context");
    client.invalidate(root.toString()); assert.equal(client.chosenRootForDocument(child), undefined); await client.modelInfo(root);
    assert.equal(client.chosenRootForDocument(child).toString(), root.toString(), "fresh proof restores an explicit choice");
  } finally { await client.shutdown(); }
});
test("multi-owner presentation waits for a user pick before workspace priority is explicit", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), child = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, { uri: b, languageId: "dynare", version: 1 }, child);
  const related = [{ kind: "include", filename: "shared.inc", resolved: true, path: child.uri.fsPath }];
  host.managed[0].client.sendRequest = (_method, params) => Promise.resolve(model(params.arguments[0].root_uri, related));
  try {
    await client.modelInfo(a); await client.modelInfo(b);
    assert.equal(await client.rootForDocument(child, false), undefined); assert.equal(client.chosenRootForDocument(child), undefined); assert.equal(host.picks.length, 0);
    host.pick = items => items.find(item => item.root.toString() === b.toString());
    assert.equal((await client.rootForDocument(child, true)).toString(), b.toString()); assert.equal(host.picks.length, 1);
    assert.equal(client.chosenRootForDocument(child).toString(), b.toString());
    assert.equal((await client.rootForDocument(child, false)).toString(), b.toString()); assert.equal(host.picks.length, 1);
  } finally { await client.shutdown(); }
});

// Guards a real owner-picker cancellation being converted to a refusal toast.
test("cancelling an owner picker stays silent at the command boundary", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod");
  const child = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, { uri: b, languageId: "dynare", version: 1 }, child);
  const related = [{ kind: "include", filename: "shared.inc", resolved: true, path: child.uri.fsPath }];
  host.managed[0].client.sendRequest = (_method, params) => Promise.resolve(model(params.arguments[0].root_uri, related));
  await client.modelInfo(a); await client.modelInfo(b);
  const before = host.notifications.length; host.pick = () => undefined;
  await runCommand(client, () => client.rootForDocument(child, true));
  assert.equal(host.picks.length, 1);
  assert.equal(host.notifications.length, before);
  assert.equal(client.chosenRootForDocument(child), undefined);
  await client.shutdown();
});
test("native navigation upgrades automatic provenance and close, restart and folders clear explicit context", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), child = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: root, languageId: "dynare", version: 1 }, child);
  const related = [{ kind: "include", filename: "shared.inc", resolved: true, path: child.uri.fsPath }];
  const reply = (_method, params) => Promise.resolve({ ...model(root.toString(), related), document_uri: params.arguments[0].document_uri });
  host.managed[0].client.sendRequest = reply;
  try {
    await client.modelInfo(root); await client.rootForDocument(child, false); assert.equal(client.chosenRootForDocument(child), undefined);
    const location = { uri: child.uri.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
    await client.openLocation(location, root); const loaded = host.documents.at(-1);
    assert.equal(client.chosenRootForDocument(loaded).toString(), root.toString(), "accepted native source navigation is explicit");
    await client.restart(); assert.equal(client.chosenRootForDocument(loaded), undefined, "restart withdraws old proof");
    host.managed.at(-1).client.sendRequest = reply; await client.modelInfo(root);
    assert.equal(client.chosenRootForDocument(loaded), undefined, "restart clears the explicit choice even after new ownership proof");
    assert.equal((await client.rootForDocument(loaded)).toString(), root.toString(), "unique ownership is rediscovered automatically");
    loaded.isClosed = true; host.close.fire(loaded);
    const reopened = { ...loaded, isClosed: false }; host.documents.push(reopened); await client.modelInfo(root);
    assert.equal((await client.rootForDocument(reopened, false)).toString(), root.toString(), "close preserves existing presentation convenience");
    assert.equal(client.chosenRootForDocument(reopened), undefined, "reopening does not inherit explicit priority");
    client.selectOwner(reopened.uri, root); assert.equal(client.chosenRootForDocument(reopened).toString(), root.toString());
    const removed = host.folders; host.folders = [{ uri: Uri.parse("file:///other"), name: "other" }];
    host.folder.fire({ added: host.folders, removed }); await client.modelInfo(root);
    assert.equal(client.chosenRootForDocument(reopened), undefined, "folder-set change clears the explicit choice");
    await client.rootForDocument(reopened, false); assert.equal(client.chosenRootForDocument(reopened), undefined, "ambient rediscovery stays automatic after folder clearing");
  } finally { await client.shutdown(); }
});
test("ordinary and untitled roots need no owner proof while includes and retained mod fragments cannot guess one", async () => {
  reset(); const client = service();
  client.modelInfo = () => { throw new Error("no model lookup"); }; client.ownerChoices = () => { throw new Error("no owner discovery"); };
  for (const value of ["file:///workspace/root.mod", "file:///outside/loose.DYN", "untitled:/scratch.mod"]) {
    const doc = { uri: Uri.parse(value), languageId: "dynare", version: 1 };
    assert.equal(client.chosenRootForDocument(doc).toString(), value);
    assert.equal(client.chosenRootForDocument({ ...doc, isClosed: true }), undefined);
  }
  const owner = Uri.parse("file:///workspace/owner.mod"), fragment = { uri: Uri.parse("file:///workspace/fragment.mod"), languageId: "dynare", version: 1 };
  assert.equal(client.chosenRootForDocument({ uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare" }), undefined);
  assert.equal(client.chosenRootForDocument({ uri: Uri.parse("dygnosis-effective:/root.mod"), languageId: "dynare" }), undefined);
  assert.equal(client.chosenRootForDocument({ uri: fragment.uri, languageId: "plaintext" }), undefined);
  client.selectOwner(fragment.uri, owner); assert.equal(client.chosenRootForDocument(fragment), undefined);
  client.treatAsRoot(fragment.uri); assert.equal(client.chosenRootForDocument(fragment).toString(), fragment.uri.toString());
  await client.shutdown();
});
test("late model-info proofs cannot announce a wrong owner or restore invalidated inputs", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///workspace/a.mod"), b = Uri.parse("file:///workspace/b.mod"), child = { uri: Uri.parse("file:///workspace/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: a, languageId: "dynare", version: 1 }, { uri: b, languageId: "dynare", version: 1 }, child);
  const related = [{ kind: "include", filename: "shared.inc", resolved: true, path: child.uri.fsPath }];
  const old = deferred(), choices = []; let updates = 0;
  host.managed[0].client.sendRequest = (_method, params) => params.arguments[0].root_uri === a.toString() ? old.promise : Promise.resolve(model(b.toString(), related));
  const listener = client.onDidUpdateModelInfo(() => { ++updates; choices.push(client.chosenRootForDocument(child)?.toString()); });
  client.selectOwner(child.uri, a); const first = client.modelInfo(a); await flush();
  client.selectOwner(child.uri, b); await client.modelInfo(b);
  old.resolve(model(a.toString(), related)); await first;
  assert.deepEqual(choices, [b.toString(), b.toString()], "an old owner's valid response reads the newer selected context");
  const obsolete = deferred(); host.managed[0].client.sendRequest = () => obsolete.promise;
  const stale = client.modelInfo(b, b, true); await flush(); client.invalidate();
  obsolete.resolve(model(b.toString(), related)); assert.equal(await stale, undefined);
  assert.equal(updates, 2); assert.equal(client.chosenRootForDocument(child), undefined);
  listener.dispose(); await client.shutdown();
});

test("input invalidation has its own root event and excludes owner presentation changes", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const events = [], presentation = [];
  const input = client.onDidInvalidate(event => events.push(event));
  const changed = client.onDidChange(() => presentation.push("changed"));
  const root = Uri.parse("file:///workspace/root.mod"), include = Uri.parse("file:///workspace/part.inc");
  client.selectOwner(include, root); client.treatAsRoot(include);
  assert.deepEqual(events, []); assert.equal(presentation.length, 2);
  client.invalidate(root.toString()); client.invalidate();
  assert.deepEqual(events, [{ root: root.toString(), reason: "input" }, { root: undefined, reason: "input" }]); assert.equal(presentation.length, 4);
  input.dispose(); changed.dispose(); await client.shutdown();
});

test("overlapping fresh model reads serialize and ordinary presentation still reuses the cache", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const firstReply = deferred(), secondReply = deferred(), replies = [firstReply, secondReply], tokens = [];
  host.managed[0].client.sendRequest = (_method, _params, token) => { tokens.push(token); return replies.shift().promise; };
  const first = client.modelInfo(root, root, true), second = client.modelInfo(root, root, true);
  await flush(); assert.equal(tokens.length, 1); assert.equal(tokens[0].isCancellationRequested, false);
  const cached = client.modelInfo(root); await flush(); assert.equal(tokens.length, 1);
  firstReply.resolve(model()); const firstSnapshot = await first;
  assert.equal(firstSnapshot.revision, "one"); assert.equal(await cached, firstSnapshot);
  await flush(); assert.equal(tokens.length, 2);
  secondReply.resolve({ ...model(), revision: "two" }); const secondSnapshot = await second;
  assert.equal(secondSnapshot.revision, "two"); assert.equal(secondSnapshot.client_instance, client.currentInstance);
  assert.equal((await client.modelInfo(root)).revision, "two"); assert.equal(tokens.length, 2);
  await client.shutdown();
});

test("two concurrent source revalidations both obtain the current same-root snapshot", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const firstReply = deferred(), secondReply = deferred(), replies = [firstReply, secondReply]; let requests = 0;
  host.managed[0].client.sendRequest = () => { ++requests; return replies.shift().promise; };
  const first = client.revalidate(root, "one", client.currentInstance), second = client.revalidate(root, "one", client.currentInstance);
  await flush(); assert.equal(requests, 1);
  firstReply.resolve(model()); assert.equal((await first).revision, "one"); await flush(); assert.equal(requests, 2);
  secondReply.resolve(model()); assert.equal((await second).revision, "one");
  await client.shutdown();
});

test("fresh queues for different documents do not block each other", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), include = Uri.parse("file:///workspace/part.inc");
  host.documents.push({ uri: root, languageId: "dynare", version: 1 }, { uri: include, languageId: "dynare", version: 1 });
  const requests = [];
  host.managed[0].client.sendRequest = (_method, params) => { const reply = deferred(); requests.push({ params, reply }); return reply.promise; };
  const rootRead = client.modelInfo(root, root, true), includeRead = client.modelInfo(root, include, true); await flush();
  assert.equal(requests.length, 2);
  for (const request of requests) request.reply.resolve({ ...model(), document_uri: request.params.arguments[0].document_uri });
  assert.equal((await rootRead).document_uri, root.toString()); assert.equal((await includeRead).document_uri, include.toString());
  await client.shutdown();
});

test("invalidation releases active and queued fresh reads without waiting for an obsolete reply", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), document = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(document);
  const oldReply = deferred(); let requests = 0, oldToken;
  host.managed[0].client.sendRequest = (_method, _params, token) => { ++requests; oldToken = token; return oldReply.promise; };
  const first = client.modelInfo(root, root, true), queued = client.revalidate(root, "one", client.currentInstance); await flush();
  assert.equal(requests, 1); ++document.version; client.invalidate(root.toString());
  assert.deepEqual(await Promise.all([first, queued]), [undefined, undefined]); assert.equal(oldToken.isCancellationRequested, true);
  await flush(); assert.equal(requests, 1, "the queued old epoch must never issue a request");
  host.managed[0].client.sendRequest = () => { ++requests; return Promise.resolve({ ...model(), document_version: 2, revision: "two" }); };
  const current = await client.modelInfo(root, root, true); assert.equal(current.revision, "two"); assert.equal(requests, 2);
  oldReply.resolve(model()); await flush(); assert.equal((await client.modelInfo(root)).revision, "two");
  await client.shutdown();
});

test("restart releases queued fresh reads and only the replacement instance can revalidate", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const oldReply = deferred(); let requests = 0;
  host.managed[0].client.sendRequest = () => { ++requests; return oldReply.promise; };
  const oldInstance = client.currentInstance;
  const first = client.revalidate(root, "one", oldInstance), queued = client.revalidate(root, "one", oldInstance); await flush();
  await client.restart(); assert.deepEqual(await Promise.all([first, queued]), [undefined, undefined]); assert.equal(requests, 1);
  assert.equal(await client.revalidate(root, "one", oldInstance), undefined);
  const current = await client.revalidate(root, "one", client.currentInstance); assert.equal(current.client_instance, client.currentInstance);
  oldReply.resolve(model()); await flush(); assert.equal((await client.modelInfo(root)).client_instance, client.currentInstance);
  await client.shutdown();
});

test("a queued fresh read captures its document version before waiting for its predecessor", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"), document = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(document);
  const reply = deferred(); let requests = 0;
  host.managed[0].client.sendRequest = () => { ++requests; return reply.promise; };
  const first = client.modelInfo(root, root, true), queued = client.modelInfo(root, root, true); await flush();
  ++document.version; reply.resolve(model());
  assert.deepEqual(await Promise.all([first, queued]), [undefined, undefined]);
  assert.equal(requests, 1, "an old queued source version cannot become a new query");
  await client.shutdown();
});

test("shutdown releases a fresh queue even if the active server request ignores cancellation", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const ignored = deferred(); host.managed[0].client.sendRequest = () => ignored.promise;
  const first = client.modelInfo(root, root, true), queued = client.modelInfo(root, root, true); await flush();
  await client.shutdown(); assert.deepEqual(await Promise.all([first, queued]), [undefined, undefined]);
  assert.equal(host.managed[0].disposed, true);
  ignored.resolve(model()); await flush(); assert.equal(await client.modelInfo(root, root, true), undefined);
});

test("native Beside placement is forwarded only after a source loader guard accepts", async () => {
  reset(); const client = service(), pending = deferred();
  const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/part.inc");
  const loaded = { uri: target, languageId: "dynare", version: 1 }, originalOpen = vscode.workspace.openTextDocument;
  const location = { uri: target.toString(), range: { start: { line: 2, character: 3 }, end: { line: 2, character: 5 } } };
  vscode.workspace.openTextDocument = () => pending.promise;
  const rejected = client.openLocation(location, root, () => false, { viewColumn: vscode.ViewColumn.Beside });
  pending.resolve(loaded); await assert.rejects(rejected, /source changed/); assert.equal(host.reveals.length, 0);
  await client.openLocation(location, root, document => document === loaded, { viewColumn: vscode.ViewColumn.Beside });
  assert.equal(host.reveals.length, 1); assert.equal(host.reveals[0].options.viewColumn, vscode.ViewColumn.Beside);
  assert.deepEqual(host.reveals[0].options.selection.coordinates, [2, 3, 2, 5]);
  vscode.workspace.openTextDocument = originalOpen; await client.shutdown();
});

test("reuseOpen prefers a visible editor after validation and otherwise reuses a hidden tab", async () => {
  reset(); const client = service();
  const root = Uri.parse("file:///workspace/root.mod"), target = Uri.parse("file:///workspace/part.inc");
  const loaded = { uri: target, languageId: "dynare", version: 1 };
  const location = { uri: target.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
  host.documents.push(loaded);
  host.visibleEditors = [{ document: loaded, viewColumn: 1 }];
  host.editor = { document: { uri: root }, viewColumn: 1 };
  await client.openLocation(location, root, () => true, { reuseOpen: true });
  assert.equal(host.reveals[0].options.viewColumn, 1);
  assert.equal(host.reveals[0].options.reuseOpen, undefined);
  host.reveals = []; host.visibleEditors = [];
  host.tabGroups = [{ viewColumn: 3, tabs: [{ input: new TabInputText(target) }] }];
  await client.openLocation(location, root, () => true, { reuseOpen: true });
  assert.equal(host.reveals[0].options.viewColumn, 3);
  host.reveals = []; host.tabGroups = [];
  await client.openLocation(location, root, () => true, { reuseOpen: true });
  assert.equal(host.reveals[0].options.viewColumn, vscode.ViewColumn.Beside);
  await client.shutdown();
});

test("dependency patterns match literal bracket/brace names and ignore unrelated directory events", async () => {
  for (const filename of ["[part].macro", "{part,other}.macro"]) for (const event of ["created", "disk", "deleted"]) {
    reset(); const client = service(); await client.ensureStarted();
    const root = Uri.parse("file:///workspace/root.mod"), candidate = Uri.parse(`file:///workspace/${filename}`);
    host.documents.push({ uri: root, languageId: "dynare", version: 1 });
    host.managed[0].client.sendRequest = () => Promise.resolve({ ...model(), dependency_candidates: [root.toString(), candidate.toString()] });
    await client.modelInfo(root);
    const dependencyPatterns = host.watchers.filter(watcher => typeof watcher.pattern !== "string").map(watcher => watcher.pattern);
    assert.ok(dependencyPatterns.every(pattern => pattern.pattern === "*" && matches(pattern, candidate)));
    let invalidations = 0; const listener = client.onDidInvalidate(() => { ++invalidations; });
    host[event].fire(Uri.parse("file:///workspace/unrelated.macro")); assert.equal(invalidations, 0);
    host[event].fire(candidate); assert.equal(invalidations, 1, `${filename} ${event}`);
    assert.ok(host.notifications.some(notification => notification.method === "workspace/didChangeWatchedFiles" && notification.params.changes[0].uri === candidate.toString()));
    listener.dispose(); await client.shutdown();
  }
});

test("caller cancellation releases an active proof while two independent callers remain live", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const calls = [];
  host.managed[0].client.sendRequest = (_method, _params, token) => { const reply = deferred(); calls.push({ token, reply }); return reply.promise; };
  const obsolete = new CancellationTokenSource(), second = new CancellationTokenSource(), third = new CancellationTokenSource();
  const oldProof = client.revalidate(root, "one", client.currentInstance, root, obsolete.token);
  const secondProof = client.revalidate(root, "one", client.currentInstance, root, second.token);
  const thirdProof = client.revalidate(root, "one", client.currentInstance, root, third.token);
  await flush(); assert.equal(calls.length, 1);
  obsolete.cancel(); assert.equal(await oldProof, undefined); await flush();
  assert.equal(calls[0].token.isCancellationRequested, true); assert.equal(calls.length, 2);
  assert.equal(calls[1].token.isCancellationRequested, false); assert.equal(second.token.isCancellationRequested, false); assert.equal(third.token.isCancellationRequested, false);
  calls[1].reply.resolve(model()); assert.equal((await secondProof).revision, "one"); await flush(); assert.equal(calls.length, 3);
  calls[2].reply.resolve(model()); assert.equal((await thirdProof).revision, "one");
  assert.equal(obsolete.cancelled.listeners.size, 0); assert.equal(second.cancelled.listeners.size, 0); assert.equal(third.cancelled.listeners.size, 0);
  calls[0].reply.resolve(model()); await client.shutdown();
});

test("cancelling a queued proof returns immediately without letting a later caller cancel its live predecessor", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///workspace/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const calls = [];
  host.managed[0].client.sendRequest = (_method, _params, token) => { const reply = deferred(); calls.push({ token, reply }); return reply.promise; };
  const first = new CancellationTokenSource(), queued = new CancellationTokenSource(), last = new CancellationTokenSource();
  const firstProof = client.revalidate(root, "one", client.currentInstance, root, first.token);
  const cancelledProof = client.revalidate(root, "one", client.currentInstance, root, queued.token);
  const lastProof = client.revalidate(root, "one", client.currentInstance, root, last.token);
  await flush(); assert.equal(calls.length, 1); queued.cancel(); assert.equal(await cancelledProof, undefined); await flush();
  assert.equal(calls.length, 1); assert.equal(calls[0].token.isCancellationRequested, false);
  assert.equal(queued.cancelled.listeners.size, 0);
  calls[0].reply.resolve(model()); assert.equal((await firstProof).revision, "one"); await flush(); assert.equal(calls.length, 2);
  assert.equal(calls[1].token.isCancellationRequested, false); calls[1].reply.resolve(model()); assert.equal((await lastProof).revision, "one");
  await client.shutdown();
});

test("Windows canonical loaded URI retains the chosen owner among several include roots", { skip: process.platform !== "win32" }, async () => {
  reset(); const client = service(); await client.ensureStarted();
  const first = Uri.parse("file:///workspace/first.mod"), chosen = Uri.parse("file:///workspace/chosen.mod"), target = Uri.parse("file:///C:/Models/shared.inc");
  const loaded = { uri: Uri.parse("file:///c%3A/Models/shared.inc"), languageId: "dynare", version: 1 };
  host.documents.push({ uri: first, languageId: "dynare", version: 1 }, { uri: chosen, languageId: "dynare", version: 1 });
  host.managed[0].client.sendRequest = (_method, params) => Promise.resolve(model(params.arguments[0].root_uri,
    [{ kind: "include", filename: "shared.inc", resolved: true, path: loaded.uri.fsPath }]));
  await client.modelInfo(first); await client.modelInfo(chosen); assert.equal(client.knownOwners(loaded.uri).length, 2);
  client.selectOwner(loaded.uri, first);
  const originalOpen = vscode.workspace.openTextDocument;
  vscode.workspace.openTextDocument = () => Promise.resolve(loaded);
  try {
    const location = { uri: target.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
    await assert.rejects(client.openLocation(location, chosen, () => false), /source changed/);
    assert.equal((await client.rootForDocument(loaded)).toString(), first.toString(), "a rejected load cannot change the native document owner");
    await client.openLocation(location, chosen, document => document === loaded);
    assert.equal((await client.rootForDocument(loaded)).toString(), chosen.toString(), "the accepted explicit root belongs to the canonical loaded URI");
  } finally { vscode.workspace.openTextDocument = originalOpen; await client.shutdown(); }
});

test("empty-content document metadata changes do not invalidate model inputs", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const source = { uri: Uri.parse("file:///workspace/root.mod"), languageId: "dynare", version: 1 };
  const events = []; const listener = client.onDidInvalidate(event => events.push(event));
  host.edit.fire({ document: source, contentChanges: [] }); assert.equal(events.length, 0);
  host.edit.fire({ document: source, contentChanges: [{ text: "var y;" }] });
  assert.equal(events[0].reason, "file"); assert.equal(events[0].uri, source.uri.toString());
  host.disk.fire(source.uri); assert.equal(events[1].reason, "file"); assert.equal(events[1].uri, source.uri.toString());
  listener.dispose(); await client.shutdown();
});

test("a guard decision is checked after its await before source owner or editor changes", async () => {
  reset(); const client = service();
  const first = Uri.parse("file:///workspace/first.mod"), chosen = Uri.parse("file:///workspace/chosen.mod"), target = Uri.parse("file:///workspace/shared.inc");
  const loaded = { uri: target, languageId: "dynare", version: 1 }, originalOpen = vscode.workspace.openTextDocument;
  client.modelInfo = root => Promise.resolve(model(root.toString(), [{ kind: "include", filename: "shared.inc", resolved: true, path: target.fsPath }]));
  client.selectOwner(target, first); vscode.workspace.openTextDocument = () => Promise.resolve(loaded);
  try {
    const location = { uri: target.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
    let current = true;
    await assert.rejects(client.openLocation(location, chosen, async () => {
      void Promise.resolve().then(() => { current = false; });
      return { isCurrent: () => current };
    }), /source changed/);
    assert.equal(host.reveals.length, 0);
    assert.equal((await client.rootForDocument(loaded)).toString(), first.toString());
    current = true;
    await client.openLocation(location, chosen, async () => ({ isCurrent: () => current }));
    assert.equal(host.reveals.length, 1);
    assert.equal((await client.rootForDocument(loaded)).toString(), chosen.toString());
  } finally { vscode.workspace.openTextDocument = originalOpen; await client.shutdown(); }
});
