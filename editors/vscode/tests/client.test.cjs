const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");
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
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
const vscode = {
  Disposable, EventEmitter: Emitter, Uri,
  DocumentLink: class { constructor(range, target) { this.range = range; this.target = target; } },
  Range: class { constructor(...coordinates) { this.coordinates = coordinates; } },
  RelativePattern: class {},
  CancellationTokenSource: class { token = { isCancellationRequested: false }; cancel() { this.token.isCancellationRequested = true; } dispose() {} },
  window: {
    get activeTextEditor() { return host.editor; },
    createOutputChannel: () => ({ appendLine() {}, append() {}, show() {}, dispose() {} }),
    showErrorMessage: () => Promise.resolve(undefined), showQuickPick: items => Promise.resolve(items[0]),
    showTextDocument: document => { host.opened.push(document.uri.toString()); return Promise.resolve({ document }); },
  },
  commands: { registerCommand: (name, callback) => { host.commands.set(name, callback); return new Disposable(() => host.commands.delete(name)); } },
  workspace: {
    get textDocuments() { return host.documents; },
    get workspaceFolders() { return host.folders; },
    getConfiguration: () => ({ get: (key, fallback) => host.settings[key] ?? fallback, inspect: key => ({ globalValue: host.settings[key] }) }),
    createFileSystemWatcher: () => ({ onDidCreate: host.created.event, onDidChange: host.disk.event, onDidDelete: host.deleted.event, dispose() {} }),
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
const { DygnosisClient } = require("../out/client");
Module._load = originalLoad;
function reset() {
  host = { documents: [], folders: [{ uri: Uri.parse("file:///project"), name: "project" }], settings: {}, commands: new Map(), opened: [], managed: [], notifications: [], symbols: 0 };
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
test("edits and disk/server invalidations coalesce a native Outline refresh", async () => {
  reset(); const client = service(); await client.ensureStarted(); const before = host.symbols;
  const included = { uri: Uri.parse("file:///project/shared.inc"), languageId: "dynare", version: 2 };
  host.edit.fire({ document: included }); host.disk.fire(included.uri);
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
    if (changed === "version") { source.version = 2; host.edit.fire({ document: source }); }
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
