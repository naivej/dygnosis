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
const vscode = {
  Disposable, EventEmitter: Emitter, Uri, TabInputText, ViewColumn: { Beside: 2, One: 1 },
  DocumentLink: class { constructor(range, target) { this.range = range; this.target = target; } },
  Range: class { constructor(...coordinates) { this.coordinates = coordinates; } },
  RelativePattern: class { constructor(base, pattern) { this.base = base; this.pattern = pattern; } }, CancellationTokenSource,
  window: {
    get activeTextEditor() { return host.editor; },
    get visibleTextEditors() { return host.visibleEditors ?? (host.editor ? [host.editor] : []); },
    tabGroups: { get all() { return host.tabGroups ?? []; } },
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
const languageClient = { DocumentSymbolRequest: { method: "textDocument/documentSymbol" }, ExecuteCommandRequest: { type: "workspace/executeCommand" }, State: { Stopped: 1 } };
const originalLoad = Module._load;
Module._load = function(id, ...args) { return id === "vscode" ? vscode : id === "vscode-languageclient/node" ? languageClient : originalLoad.call(this, id, ...args); };
const { DygnosisClient, extendCapabilities } = require("../out/client");
Module._load = originalLoad;
function reset() {
  host = { documents: [], folders: [{ uri: Uri.parse("file:///project"), name: "project" }], settings: {}, commands: new Map(), opened: [], reveals: [], managed: [], watchers: [], notifications: [], picks: [], symbols: 0,
    visibleEditors: undefined, tabGroups: [] };
  for (const name of ["created", "disk", "deleted", "open", "edit", "close", "folder", "config"]) host[name] = new Emitter();
}
function model(root = "file:///project/root.mod", related = []) {
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
          experimental: { dygnosis: { modelInfo: { schema_version: 1 } } }, documentSymbolProvider: true,
          executeCommandProvider: { commands: ["dynare/modelInfo", "dynare/showEffectiveModel"] },
        } },
        sendNotification: (method, params) => { host.notifications.push({ method, params }); return options.notify?.(method, params) ?? Promise.resolve(); },
        onNotification: (_method, listener) => { client.notify = listener; return new Disposable(); },
        onDidChangeState: listener => { client.state = listener; return new Disposable(); },
        getFeature: () => ({ clear() {}, unregister() {}, register() { ++host.symbols; } }),
        sendRequest: () => Promise.resolve(model()),
      };
      const managed = { client, disposed: false,
        start: () => { capture({ workspaceFolders: host.folders.map(folder => ({ uri: folder.uri.toString(), name: folder.name })), capabilities: {}, initializationOptions: settings }); if (!options.pending) initialized.resolve(); return initialized.promise; },
        dispose: () => { managed.disposed = true; initialized.resolve(); return Promise.resolve(); }, initialized,
      };
      host.managed.push(managed); return managed;
    },
  });
}
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
  host.managed[0].client.notify({ schema_version: 1, root_uri: "file:///project/root.mod", revision: "model-token", input_revision: "diagnostic-token" });
  assert.equal(events[0].inputRevision, "diagnostic-token");
  assert.equal(events[0].root, "file:///project/root.mod");
  listener.dispose(); await client.shutdown();
});
test("edits and disk/server invalidations coalesce a native Outline refresh", async () => {
  reset(); const client = service(); await client.ensureStarted(); const before = host.symbols;
  const included = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 2 };
  host.edit.fire({ document: included, contentChanges: [{ text: "changed" }] }); host.disk.fire(included.uri);
  host.managed[0].client.notify({ schema_version: 1, root_uri: "file:///project/root.mod" });
  await new Promise(resolve => setTimeout(resolve, 110));
  assert.equal(host.symbols, before + 1); await client.shutdown();
});
test("unavailable owner responses preserve a selected owner until current facts disprove it", async () => {
  reset(); const client = service(); const root = Uri.parse("file:///project/root.mod"), child = { uri: Uri.parse("file:///project/fragment.mod"), languageId: "dynare", version: 1 };
  client.selectOwner(child.uri, root);
  client.modelInfo = () => Promise.resolve(undefined);
  assert.equal(await client.rootForDocument(child), undefined);
  client.modelInfo = () => Promise.resolve(model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: child.uri.fsPath }]));
  assert.equal((await client.rootForDocument(child)).toString(), root.toString());
  client.modelInfo = () => Promise.resolve(model());
  assert.equal(await client.rootForDocument(child), undefined, "removed edge must not promote the fragment to a root");
  client.treatAsRoot(child.uri);
  assert.equal((await client.rootForDocument(child)).toString(), child.uri.toString()); await client.shutdown();
});
test("a newer selected owner survives both an old owner reply and an old ownership disproof", async () => {
  for (const stillOwned of [false, true]) {
    reset(); const client = service(), pending = deferred();
    const a = Uri.parse("file:///project/a.mod"), b = Uri.parse("file:///project/b.mod");
    const child = { uri: Uri.parse("file:///project/fragment.mod"), languageId: "dynare", version: 1 };
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
  const root = Uri.parse("file:///project/root.mod");
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
  await client.restart(); assert.equal(old.disposed, true); assert.equal(client.client, undefined); await client.shutdown();
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
test("native include links route through the retained owner before opening a mod fragment", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/fragment.mod");
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
  const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/fragment.mod");
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
    const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/fragment.mod");
    const source = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(source); host.editor = { document: source };
    client.modelInfo = () => Promise.resolve({ ...model(root.toString(), [{ kind: "include", filename: "fragment.mod", resolved: true, path: target.fsPath }]), client_instance: client.currentInstance });
    const first = await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, target)]);
    const [key] = JSON.parse(decodeURIComponent(first[0].target.query));
    if (changed === "version") { source.version = 2; host.edit.fire({ document: source, contentChanges: [{ text: "changed" }] }); }
    else if (changed === "close") host.close.fire(source);
    else if (changed === "target") await client.middleware.provideDocumentLinks(source, { isCancellationRequested: false }, () => [new vscode.DocumentLink({}, Uri.parse("file:///project/replacement.mod"))]);
    else await client.restart();
    await host.commands.get("dygnosis.openIncludeLink")(key);
    assert.deepEqual(host.opened, [], `${changed} must invalidate the saved command`);
    await client.shutdown();
  }
});
test("native include validation refuses a newer owner or editor selected while its facts are pending", async () => {
  for (const changed of ["owner", "editor"]) {
    reset(); const client = service(); await client.ensureStarted(); const pending = deferred();
    const a = Uri.parse("file:///project/a.mod"), b = Uri.parse("file:///project/b.mod"), target = Uri.parse("file:///project/fragment.mod");
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
  const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/fragment.mod");
  const source = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 1 };
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
  const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/fragment.mod");
  const loaded = { uri: target, languageId: "dynare", version: 1 };
  const originalOpen = vscode.workspace.openTextDocument;
  vscode.workspace.openTextDocument = () => pending.promise;
  let current = true, sawLoaded = false;
  const location = { uri: target.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
  const jump = client.openLocation(location, root, document => { sawLoaded = document === loaded; return current; });
  current = false; pending.resolve(loaded); await jump;
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
  assert.equal(capabilities.experimental.dygnosis.projectStatusChanged, true);
});
test("chosen context reads only a proven selected owner and restores it on a separate model-info event", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///project/a.mod"), b = Uri.parse("file:///project/b.mod");
  const child = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 1 };
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
test("ambient sole-owner include presentation does not gain explicit project priority", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///project/root.mod"), child = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 1 };
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
test("multi-owner presentation waits for a user pick before project priority is explicit", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///project/a.mod"), b = Uri.parse("file:///project/b.mod"), child = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 1 };
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
test("native navigation upgrades automatic provenance, close clears priority and restart requires restored proof", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///project/root.mod"), child = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 1 };
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
    assert.equal(client.chosenRootForDocument(loaded).toString(), root.toString(), "restart preserves explicit intent after fresh proof");
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
  for (const value of ["file:///project/root.mod", "file:///outside/loose.DYN", "untitled:/scratch.mod"]) {
    const doc = { uri: Uri.parse(value), languageId: "dynare", version: 1 };
    assert.equal(client.chosenRootForDocument(doc).toString(), value);
    assert.equal(client.chosenRootForDocument({ ...doc, isClosed: true }), undefined);
  }
  const owner = Uri.parse("file:///project/owner.mod"), fragment = { uri: Uri.parse("file:///project/fragment.mod"), languageId: "dynare", version: 1 };
  assert.equal(client.chosenRootForDocument({ uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare" }), undefined);
  assert.equal(client.chosenRootForDocument({ uri: Uri.parse("dygnosis-effective:/root.mod"), languageId: "dynare" }), undefined);
  assert.equal(client.chosenRootForDocument({ uri: fragment.uri, languageId: "plaintext" }), undefined);
  client.selectOwner(fragment.uri, owner); assert.equal(client.chosenRootForDocument(fragment), undefined);
  client.treatAsRoot(fragment.uri); assert.equal(client.chosenRootForDocument(fragment).toString(), fragment.uri.toString());
  await client.shutdown();
});
test("late model-info proofs cannot announce a wrong owner or restore invalidated inputs", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const a = Uri.parse("file:///project/a.mod"), b = Uri.parse("file:///project/b.mod"), child = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 1 };
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
  const root = Uri.parse("file:///project/root.mod"), include = Uri.parse("file:///project/part.inc");
  client.selectOwner(include, root); client.treatAsRoot(include);
  assert.deepEqual(events, []); assert.equal(presentation.length, 2);
  client.invalidate(root.toString()); client.invalidate();
  assert.deepEqual(events, [{ root: root.toString(), reason: "input" }, { root: undefined, reason: "input" }]); assert.equal(presentation.length, 4);
  input.dispose(); changed.dispose(); await client.shutdown();
});

test("overlapping fresh model reads serialize and ordinary presentation still reuses the cache", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///project/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
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
  const root = Uri.parse("file:///project/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
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
  const root = Uri.parse("file:///project/root.mod"), include = Uri.parse("file:///project/part.inc");
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
  const root = Uri.parse("file:///project/root.mod"), document = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(document);
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
  const root = Uri.parse("file:///project/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
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
  const root = Uri.parse("file:///project/root.mod"), document = { uri: root, languageId: "dynare", version: 1 }; host.documents.push(document);
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
  const root = Uri.parse("file:///project/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
  const ignored = deferred(); host.managed[0].client.sendRequest = () => ignored.promise;
  const first = client.modelInfo(root, root, true), queued = client.modelInfo(root, root, true); await flush();
  await client.shutdown(); assert.deepEqual(await Promise.all([first, queued]), [undefined, undefined]);
  assert.equal(host.managed[0].disposed, true);
  ignored.resolve(model()); await flush(); assert.equal(await client.modelInfo(root, root, true), undefined);
});

test("native Beside placement is forwarded only after a source loader guard accepts", async () => {
  reset(); const client = service(), pending = deferred();
  const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/part.inc");
  const loaded = { uri: target, languageId: "dynare", version: 1 }, originalOpen = vscode.workspace.openTextDocument;
  const location = { uri: target.toString(), range: { start: { line: 2, character: 3 }, end: { line: 2, character: 5 } } };
  vscode.workspace.openTextDocument = () => pending.promise;
  const rejected = client.openLocation(location, root, () => false, { viewColumn: vscode.ViewColumn.Beside });
  pending.resolve(loaded); await rejected; assert.equal(host.reveals.length, 0);
  await client.openLocation(location, root, document => document === loaded, { viewColumn: vscode.ViewColumn.Beside });
  assert.equal(host.reveals.length, 1); assert.equal(host.reveals[0].options.viewColumn, vscode.ViewColumn.Beside);
  assert.deepEqual(host.reveals[0].options.selection.coordinates, [2, 3, 2, 5]);
  vscode.workspace.openTextDocument = originalOpen; await client.shutdown();
});

test("reuseOpen prefers a visible editor after validation and otherwise reuses a hidden tab", async () => {
  reset(); const client = service();
  const root = Uri.parse("file:///project/root.mod"), target = Uri.parse("file:///project/part.inc");
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
    const root = Uri.parse("file:///project/root.mod"), candidate = Uri.parse(`file:///project/${filename}`);
    host.documents.push({ uri: root, languageId: "dynare", version: 1 });
    host.managed[0].client.sendRequest = () => Promise.resolve({ ...model(), dependency_candidates: [root.toString(), candidate.toString()] });
    await client.modelInfo(root);
    const dependencyPatterns = host.watchers.filter(watcher => typeof watcher.pattern !== "string").map(watcher => watcher.pattern);
    assert.ok(dependencyPatterns.every(pattern => pattern.pattern === "*" && matches(pattern, candidate)));
    let invalidations = 0; const listener = client.onDidInvalidate(() => { ++invalidations; });
    host[event].fire(Uri.parse("file:///project/unrelated.macro")); assert.equal(invalidations, 0);
    host[event].fire(candidate); assert.equal(invalidations, 1, `${filename} ${event}`);
    assert.ok(host.notifications.some(notification => notification.method === "workspace/didChangeWatchedFiles" && notification.params.changes[0].uri === candidate.toString()));
    listener.dispose(); await client.shutdown();
  }
});

test("caller cancellation releases an active proof while two independent callers remain live", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const root = Uri.parse("file:///project/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
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
  const root = Uri.parse("file:///project/root.mod"); host.documents.push({ uri: root, languageId: "dynare", version: 1 });
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
  const first = Uri.parse("file:///project/first.mod"), chosen = Uri.parse("file:///project/chosen.mod"), target = Uri.parse("file:///C:/Models/shared.inc");
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
    await client.openLocation(location, chosen, () => false);
    assert.equal((await client.rootForDocument(loaded)).toString(), first.toString(), "a rejected load cannot change the native document owner");
    await client.openLocation(location, chosen, document => document === loaded);
    assert.equal((await client.rootForDocument(loaded)).toString(), chosen.toString(), "the accepted explicit root belongs to the canonical loaded URI");
  } finally { vscode.workspace.openTextDocument = originalOpen; await client.shutdown(); }
});

test("empty-content document metadata changes do not invalidate model inputs", async () => {
  reset(); const client = service(); await client.ensureStarted();
  const source = { uri: Uri.parse("file:///project/root.mod"), languageId: "dynare", version: 1 };
  const events = []; const listener = client.onDidInvalidate(event => events.push(event));
  host.edit.fire({ document: source, contentChanges: [] }); assert.equal(events.length, 0);
  host.edit.fire({ document: source, contentChanges: [{ text: "var y;" }] }); assert.equal(events[0].reason, "input");
  host.disk.fire(source.uri); assert.equal(events[1].reason, "file"); assert.equal(events[1].uri, source.uri.toString());
  listener.dispose(); await client.shutdown();
});

test("a guard decision is checked after its await before source owner or editor changes", async () => {
  reset(); const client = service();
  const first = Uri.parse("file:///project/first.mod"), chosen = Uri.parse("file:///project/chosen.mod"), target = Uri.parse("file:///project/shared.inc");
  const loaded = { uri: target, languageId: "dynare", version: 1 }, originalOpen = vscode.workspace.openTextDocument;
  client.modelInfo = root => Promise.resolve(model(root.toString(), [{ kind: "include", filename: "shared.inc", resolved: true, path: target.fsPath }]));
  client.selectOwner(target, first); vscode.workspace.openTextDocument = () => Promise.resolve(loaded);
  try {
    const location = { uri: target.toString(), range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } };
    let current = true;
    await client.openLocation(location, chosen, async () => {
      void Promise.resolve().then(() => { current = false; });
      return { isCurrent: () => current };
    });
    assert.equal(host.reveals.length, 0);
    assert.equal((await client.rootForDocument(loaded)).toString(), first.toString());
    current = true;
    await client.openLocation(location, chosen, async () => ({ isCurrent: () => current }));
    assert.equal(host.reveals.length, 1);
    assert.equal((await client.rootForDocument(loaded)).toString(), chosen.toString());
  } finally { vscode.workspace.openTextDocument = originalOpen; await client.shutdown(); }
});
