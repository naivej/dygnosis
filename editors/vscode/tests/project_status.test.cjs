const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");
const path = require("node:path");

class Disposable {
  constructor(callback) { this.callback = callback; }
  dispose() { this.callback?.(); this.callback = undefined; }
  static from(...values) { return new Disposable(() => values.forEach(value => value.dispose())); }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
  dispose() { this.listeners.clear(); }
}
class CancellationTokenSource {
  token = { isCancellationRequested: false };
  cancel() { this.token.isCancellationRequested = true; }
  dispose() {}
}
function uri(value) {
  const parsed = new URL(value), pathname = decodeURIComponent(parsed.pathname);
  return { scheme: parsed.protocol.slice(0, -1), path: pathname,
    fsPath: process.platform === "win32" ? pathname.replace(/^\//, "").replaceAll("/", "\\") : pathname,
    toString: () => value };
}
function file(value) {
  let pathname = value.replaceAll("\\", "/");
  if (!pathname.startsWith("/")) pathname = `/${pathname}`;
  return uri(`file://${pathname.split("/").map(encodeURIComponent).join("/").replace(/%3A/i, ":")}`);
}
const project = "file:///C:/project", other = "file:///C:/other";
function folder(value = project, name = "project") { return { uri: uri(value), name, index: 0 }; }
function document(value = `${project}/main.mod`) { return { uri: uri(value), languageId: "dynare", version: 1, isClosed: false }; }
function deferred() { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; }
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
const vscode = {
  Disposable, EventEmitter: Emitter, CancellationTokenSource,
  Uri: { parse: uri, file }, ThemeIcon: class { constructor(id) { this.id = id; } },
  TreeItem: class { constructor(label, collapsibleState) { this.label = label; this.collapsibleState = collapsibleState; } },
  TreeItemCollapsibleState: { None: 0, Expanded: 2 }, StatusBarAlignment: { Left: 1 },
  RelativePattern: class { constructor(baseUri, pattern) { this.baseUri = baseUri; this.pattern = pattern; } },
  ConfigurationTarget: { WorkspaceFolder: 3 },
  window: {
    get activeTextEditor() { return host.editor; },
    createStatusBarItem: (id, alignment, priority) => {
      const item = { id, alignment, priority, visible: false, disposed: false, show() { this.visible = true; }, hide() { this.visible = false; }, dispose() { this.disposed = true; } };
      host.items.push(item); return item;
    },
    createTreeView: (id, options) => {
      const view = { id, options, disposed: false, dispose() { this.disposed = true; } };
      host.views.push(view); return view;
    },
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
    showQuickPick: async (items, options) => {
      host.picks.push({ items, options });
      const choice = host.choices.shift();
      return typeof choice === "function" ? choice(items) : items.find(item => typeof item === "string" ? item === choice : item.action === choice || item.label === choice);
    },
    showInputBox: async options => { host.inputs.push(options); return host.inputValues.shift(); },
  },
  workspace: {
    get workspaceFolders() { return host.folders; },
    asRelativePath: (value, includeFolder) => {
      host.relativePaths.push({ value, includeFolder });
      return value.path.split("/").slice(-1)[0];
    },
    getConfiguration: (section, resource) => {
      host.scopes.push({ section, resource });
      const resolved = resource?.uri ?? resource;
      return {
        get: (key, fallback) => host.resources.get(resolved?.toString())?.[key] ?? host.settings[key] ?? fallback,
        update: async (key, value, target) => host.updates.push({ key, value, target, resource: resolved }),
      };
    },
    createFileSystemWatcher: pattern => {
      const watcher = { pattern, created: new Emitter(), changed: new Emitter(), deleted: new Emitter(), disposed: false,
        dispose() { this.disposed = true; } };
      watcher.onDidCreate = watcher.created.event; watcher.onDidChange = watcher.changed.event; watcher.onDidDelete = watcher.deleted.event;
      host.watchers.push(watcher); return watcher;
    },
    onDidChangeWorkspaceFolders: listener => host.folderChanged.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
  },
  commands: {
    registerCommand: (name, callback) => { host.commands.set(name, callback); return new Disposable(() => host.commands.delete(name)); },
    executeCommand: async (name, ...args) => { host.executed.push({ name, args }); },
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) { if (id === "vscode") return vscode; return originalLoad.call(this, id, ...args); };
const { parseProjectStatus, parseProjectCapability, projectStatusContent, registerProjectStatus } = require("../out/project_status");
Module._load = originalLoad;

function root(state = "checked", extra = {}) {
  return { root_uri: `${project}/main.mod`, state, revision: "model-input-1", errors: 0, warnings: 0, failure: null, dependency_candidates: [], ...extra };
}
function facts(roots = [root()], extra = {}) {
  const counts = { pending: 0, checking: 0, checked: 0, incomplete: 0, failed: 0, excluded: 0 };
  for (const entry of roots) ++counts[entry.state];
  const complete = counts.pending === 0 && counts.checking === 0;
  return { schema_version: 1, pass_revision: 1, enabled: true, discovery: "complete", cancelled: false, complete,
    coverage_complete: complete && !counts.incomplete && !counts.failed, counts, roots, discovery_failures: [],
    metrics: { discovery_ms: 5, analysis_ms: 20, completed_jobs: 1, reused_jobs: 0, elapsed_ms: 25 }, ...extra };
}
const capability = { schema_version: 1, status_command: "dynare/projectStatus", recheck_command: "dynare/recheckProject", cancel_command: "dynare/cancelProject",
  active_model_notification: "dynare/activeModelChanged", status_notification: "dynare/projectStatusChanged", typing_pause_ms: 250 };
function initializeResult(extra = {}) { return { capabilities: { experimental: { dygnosis: { projectDiagnostics: { ...capability, ...extra } } },
  executeCommandProvider: { commands: [capability.status_command, capability.recheck_command, capability.cancel_command] } } }; }
function connection(result = initializeResult()) {
  const value = { initializeResult: result, running: true, subscriptions: new Map(), sent: [], isRunning() { return this.running; },
    onNotification(name, callback) { const emitter = this.subscriptions.get(name) ?? new Emitter(); this.subscriptions.set(name, emitter); return emitter.event(callback); },
    sendNotification: async (name, params) => { value.sent.push({ name, params }); },
    notify(status) { this.subscriptions.get(capability.status_notification)?.fire(status); } };
  return value;
}
function setup(options = {}) {
  host = { folders: options.folders ?? [folder()], settings: options.settings ?? {}, resources: new Map(), scopes: [],
    editor: options.editor, items: [], views: [], watchers: [], picks: [], choices: [], inputs: [], inputValues: [], updates: [], executed: [], relativePaths: [], commands: new Map(),
    active: new Emitter(), folderChanged: new Emitter(), configured: new Emitter(), edited: new Emitter(), closed: new Emitter() };
  const changed = new Emitter();
  const service = { client: options.client, currentInstance: 1, starts: 0, queries: [], logged: [], status: options.status ?? facts(), onDidChange: changed.event,
    log: message => service.logged.push(message),
    ensureStarted: async () => { if (!service.client) { ++service.starts; service.client = connection(); changed.fire(); } },
    execute: async (command, args, token) => { service.queries.push({ command, args, token }); return service.status; },
    chosenRootForDocument: current => current.languageId === "dynare" && /\.(mod|dyn)$/i.test(current.uri.path) ? current.uri : undefined,
  };
  const configure = (key, value) => { host.settings[key] = value; host.configured.fire({ affectsConfiguration: section => section === "dynare" || `dynare.${key}`.startsWith(section) }); };
  return { host, service, changed, configure };
}
function rows(env) { return env.host.views[0].options.treeDataProvider.getChildren(); }
function liveWatchers(env, pattern) { return env.host.watchers.filter(watcher => !watcher.disposed && (!pattern || watcher.pattern.pattern === pattern)); }

test("capability requires supported schema, exact commands, notification names and command advertisement", () => {
  assert.ok(parseProjectCapability(initializeResult()));
  assert.ok(parseProjectCapability(initializeResult({ future_field: true })));
  for (const extra of [{ schema_version: 2 }, { cancel_command: "somewhere/else" }, { active_model_notification: "other/active" }]) assert.equal(parseProjectCapability(initializeResult(extra)), undefined);
  const result = initializeResult(); result.capabilities.executeCommandProvider.commands.pop();
  assert.equal(parseProjectCapability(result), undefined);
  assert.equal(parseProjectCapability({ capabilities: {} }), undefined);
});
test("status schema checks required fields, counts and consistency but allows additions", () => {
  assert.equal(parseProjectStatus({ ...facts(), future_field: 7 }).counts.checked, 1);
  assert.equal(parseProjectStatus(facts([], { metrics: { discovery_ms: 0, analysis_ms: 0, completed_jobs: 0, reused_jobs: 0, elapsed_ms: null } })).roots.length, 0);
  const cases = [
    { schema_version: 2 }, { pass_revision: -1 }, { counts: { checked: 1 } }, { metrics: {} }, { discovery: "guessing" },
    { roots: [root("checked", { dependency_candidates: ["http://external/file"] })] }, { roots: [root(), root()] },
    { roots: [root("checked", { revision: undefined })] }, { counts: { ...facts().counts, pending: 1 } },
    { enabled: false }, { discovery_failures: [{ folder_uri: project, failure: 10 }] },
  ];
  for (const extra of cases) assert.throws(() => parseProjectStatus({ ...facts(), ...extra }), /Unsupported project status/);
  assert.throws(() => parseProjectStatus(facts([root("pending")], { complete: true })), /Unsupported/);
  assert.throws(() => parseProjectStatus(facts([root("failed")], { coverage_complete: true })), /Unsupported/);
});
test("coverage distinguishes checked with Errors, failure, incomplete, pending, cancellation, off and no roots", () => {
  const checked = projectStatusContent(parseProjectStatus(facts([root("checked", { errors: 3 })])));
  assert.match(checked.text, /1\/1 checked · 3 Errors/); assert.match(checked.tooltip, /Checked models may contain diagnostic Errors/);
  for (const state of ["incomplete", "failed"]) assert.match(projectStatusContent(parseProjectStatus(facts([root(state)]))).text, /incomplete · 0\/1 checked/);
  assert.match(projectStatusContent(parseProjectStatus(facts([root("pending")]))).text, /0\/1 finished/);
  assert.match(projectStatusContent(parseProjectStatus(facts([root("checking")]))).text, /sync~spin/);
  assert.match(projectStatusContent(parseProjectStatus(facts([root("pending")], { cancelled: true }))).text, /cancelled/);
  assert.match(projectStatusContent(parseProjectStatus(facts([], { enabled: false, discovery: "disabled", complete: false, coverage_complete: false }))).text, /off/);
  assert.match(projectStatusContent(parseProjectStatus(facts([]))).text, /no root models/);
  assert.match(projectStatusContent(parseProjectStatus(facts([], { discovery: "discovering", complete: false, coverage_complete: false }))).text, /discovering/);
});
test("default-on folder-only startup uses native coverage and folder tree without opening documents", async () => {
  const env = setup(); const registration = registerProjectStatus(env.service); await flush();
  assert.equal(env.service.starts, 1); assert.equal(env.host.editor, undefined);
  assert.equal(env.host.items[0].id, "dygnosis.projectCoverage"); assert.equal(env.host.items[0].name, "Dynare project coverage");
  assert.equal(env.host.items[0].command, "dygnosis.projectStatus"); assert.match(env.host.items[0].text, /1\/1 checked/);
  assert.match(env.host.items[0].accessibilityInformation.label, /project/);
  assert.equal(env.host.views[0].id, "dygnosis.project");
  assert.equal(rows(env)[0].label, "project"); assert.equal(rows(env)[0].contextValue, "dygnosis.projectFolder");
  assert.equal(rows(env)[0].children[0].description, "Checked · 0 Errors, 0 Warnings");
  assert.equal(rows(env)[0].children[0].command.command, "vscode.open");
  assert.ok(env.service.queries.every(query => query.args.length === 0));
  await env.host.commands.get("dygnosis.projectStatus")(); assert.equal(env.host.executed[0].name, "dygnosis.project.focus");
  registration.dispose();
});
test("off and virtual/no-folder windows do not start project work", async () => {
  for (const options of [{ settings: { projectDiagnostics: false } }, { folders: [] }, { folders: [folder("remote:/project")] }]) {
    const env = setup(options); const registration = registerProjectStatus(env.service); await flush();
    assert.equal(env.service.starts, 0); assert.equal(env.service.queries.length, 0); assert.equal(liveWatchers(env).length, 0);
    assert.match(env.host.items[0].text, options.settings ? /off/ : /no folders/); registration.dispose();
  }
});
test("failed startup is unavailable instead of indefinitely starting", async () => {
  const env = setup(); env.service.ensureStarted = async () => {};
  const registration = registerProjectStatus(env.service); await flush();
  assert.match(env.host.items[0].text, /unavailable/); assert.match(env.host.items[0].tooltip, /Restart.*Output/);
  registration.dispose();
});
test("unsupported engine stays visible with recovery and sends no project protocol messages", async () => {
  const env = setup({ client: connection(initializeResult({ schema_version: 99 })) });
  const registration = registerProjectStatus(env.service); await flush();
  assert.equal(env.service.queries.length, 0); assert.equal(env.service.client.sent.length, 0); assert.equal(liveWatchers(env).length, 0);
  assert.match(env.host.items[0].tooltip, /bundled binary.*dynare.serverPath/); registration.dispose();
});
test("folder discovery failure cannot make another folder's successful roots look fully covered", async () => {
  const env = setup({ folders: [folder(), folder(other, "other")], status: facts([root("checked", { errors: 2 })],
    { coverage_complete: false, discovery_failures: [{ folder_uri: other, failure: "Permission denied" }] }) });
  const registration = registerProjectStatus(env.service); await flush();
  assert.match(env.host.items[0].text, /incomplete/); assert.match(env.host.views[0].message, /discovery failed/);
  assert.equal(rows(env).length, 2); assert.equal(rows(env)[0].children[0].description, "Checked · 2 Errors, 0 Warnings");
  assert.equal(rows(env)[1].children[0].label, "Discovery failed"); assert.equal(rows(env)[1].children[0].tooltip, "Permission denied");
  registration.dispose();
});
test("only the chosen owner is sent for priority, including include roots, null and untitled", async () => {
  const doc = document(`${project}/fragment.inc`), env = setup({ editor: { document: doc } });
  const registration = registerProjectStatus(env.service); await flush();
  assert.deepEqual(env.service.client.sent.find(sent => sent.name === capability.active_model_notification).params, { root_uri: null });
  env.service.chosenRootForDocument = () => uri(`${other}/owner.mod`); env.changed.fire(); await flush();
  assert.deepEqual(env.service.client.sent.at(-1).params, { root_uri: `${other}/owner.mod` });
  const count = env.service.client.sent.length; env.changed.fire(); assert.equal(env.service.client.sent.length, count);
  env.host.editor = undefined; env.host.active.fire(); assert.deepEqual(env.service.client.sent.at(-1).params, { root_uri: null });
  env.host.editor = { document: document("untitled:/scratch.mod") }; env.service.chosenRootForDocument = current => current.uri;
  env.host.active.fire(); assert.deepEqual(env.service.client.sent.at(-1).params, { root_uri: "untitled:/scratch.mod" });
  registration.dispose();
});
test("exact arbitrary-extension, missing and companion candidate watches treat glob characters literally", async () => {
  const candidates = [`${other}/part%5B1%5D%7Bcopy%7D.data`, `${other}/missing.noext`, `${other}/main_steadystate.m`];
  const env = setup({ status: facts([root("checked", { dependency_candidates: candidates })]) });
  const registration = registerProjectStatus(env.service); await flush();
  const watcher = liveWatchers(env, "*")[0]; assert.equal(liveWatchers(env, "*").length, 1);
  assert.equal(watcher.pattern.baseUri.fsPath, path.dirname(uri(candidates[0]).fsPath)); assert.equal(watcher.pattern.pattern, "*");
  const before = env.service.client.sent.length; watcher.changed.fire(uri(`${other}/part1copy.data`)); await flush();
  assert.equal(env.service.client.sent.length, before);
  env.service.status = facts([root("checked", { dependency_candidates: candidates })], { pass_revision: 2 });
  watcher.created.fire(uri(candidates[0])); watcher.deleted.fire(uri(candidates[1])); watcher.changed.fire(uri(candidates[2])); await flush();
  const events = env.service.client.sent.filter(sent => sent.name === "workspace/didChangeWatchedFiles");
  assert.deepEqual(events.map(event => event.params.changes[0].type), [1, 3, 2]);
  assert.deepEqual(events.map(event => event.params.changes[0].uri), candidates);
  env.service.client.notify(facts([root("excluded")], { pass_revision: 2 })); assert.equal(liveWatchers(env, "*").length, 0);
  registration.dispose();
});
test("unchanged dependency directories reuse watchers while candidate sets update", async () => {
  const first = `${other}/a.inc`, second = `${other}/b.any`, env = setup({ status: facts([root("checked", { dependency_candidates: [first] })]) });
  const registration = registerProjectStatus(env.service); await flush();
  const watcher = liveWatchers(env, "*")[0];
  env.service.client.notify(facts([root("checked", { dependency_candidates: [second] })]));
  assert.equal(liveWatchers(env, "*")[0], watcher); assert.equal(watcher.disposed, false);
  const before = env.service.client.sent.length; watcher.changed.fire(uri(first)); await flush(); assert.equal(env.service.client.sent.length, before);
  watcher.created.fire(uri(second)); await flush(); assert.equal(env.service.client.sent.at(-1).params.changes[0].uri, second);
  registration.dispose();
});
test("root creation and deletion are watched per folder; removed folders and exclusions clear watchers", async () => {
  const env = setup({ folders: [folder(), folder(other, "other")] }); const registration = registerProjectStatus(env.service); await flush();
  assert.equal(liveWatchers(env, "**/*.mod").length, 2);
  const removed = liveWatchers(env, "**/*.mod")[1]; env.host.folders = [folder()];
  env.service.status = facts([], { pass_revision: 2 }); env.host.folderChanged.fire(); await flush();
  assert.equal(removed.disposed, true); assert.equal(rows(env).length, 1);
  const watcher = liveWatchers(env, "**/*.mod")[0]; watcher.created.fire(uri(`${project}/new.mod`)); watcher.deleted.fire(uri(`${project}/old.mod`)); await flush();
  assert.deepEqual(env.service.client.sent.filter(value => value.name === "workspace/didChangeWatchedFiles").map(value => value.params.changes[0].type), [1, 3]);
  registration.dispose();
});
test("newer notifications and passes reject obsolete query responses", async () => {
  const pending = deferred(), env = setup({ client: connection() }); env.service.execute = async () => pending.promise;
  const registration = registerProjectStatus(env.service); await flush();
  env.service.client.notify(facts([root("checked", { errors: 4 })], { pass_revision: 3 }));
  pending.resolve(facts([root("pending")], { pass_revision: 1 })); await flush();
  assert.match(env.host.items[0].text, /4 Errors/);
  env.service.client.notify(facts([root("pending")], { pass_revision: 2 })); assert.match(env.host.items[0].text, /4 Errors/);
  registration.dispose();
});
for (const action of ["edit", "Recheck", "settings"]) {
  test(`${action} retains a same-pass completed notification received before its delayed pending reply`, async () => {
    const env = setup(), registration = registerProjectStatus(env.service); await flush();
    const pending = deferred(); env.service.execute = async () => pending.promise;
    const trigger = async () => {
      if (action === "edit") { env.host.edited.fire({ document: document() }); await new Promise(resolve => setTimeout(resolve, 90)); }
      else if (action === "Recheck") env.host.commands.get("dygnosis.recheckProject")();
      else env.configure("projectExcludePaths", ["generated/**"]);
      await flush();
    };
    await trigger();
    env.service.client.notify(facts([root("checked", { errors: 4 })], { pass_revision: 2 }));
    pending.resolve(facts([root("pending")], { pass_revision: 2 })); await flush();
    assert.match(env.host.items[0].text, /1\/1 checked · 4 Errors/);
    assert.equal(rows(env)[0].children[0].description, "Checked · 4 Errors, 0 Warnings");
    assert.doesNotMatch(env.host.items[0].text, /0\/1 finished/);
    const later = deferred(); env.service.execute = async () => later.promise; await trigger();
    env.service.client.notify(facts([root("checked", { errors: 99 })], { pass_revision: 2 }));
    later.resolve(facts([root("pending")], { pass_revision: 3 })); await flush();
    assert.match(env.host.items[0].text, /0\/1 finished/); assert.doesNotMatch(env.host.items[0].text, /99|4 Errors/);
    registration.dispose();
  });
}
test("refresh discards pre-action passes and superseded-epoch notification buffers", async () => {
  const env = setup(), registration = registerProjectStatus(env.service); await flush();
  const first = deferred(), second = deferred(); env.service.execute = async () => first.promise;
  env.host.commands.get("dygnosis.recheckProject")(); await flush();
  env.service.client.notify(facts([root("checked", { errors: 99 })], { pass_revision: 1 }));
  env.service.client.notify(facts([root("checked", { errors: 4 })], { pass_revision: 2 }));
  env.service.execute = async () => second.promise; env.configure("projectExcludePaths", ["generated/**"]); await flush();
  first.resolve(facts([root("pending")], { pass_revision: 2 }));
  second.resolve(facts([root("pending")], { pass_revision: 3 })); await flush();
  assert.match(env.host.items[0].text, /0\/1 finished/); assert.doesNotMatch(env.host.items[0].text, /99|4 Errors/);
  assert.equal(rows(env)[0].children[0].description, "Pending"); registration.dispose();
});
test("an acknowledged later pass supersedes an earlier buffered completion", async () => {
  const env = setup(), registration = registerProjectStatus(env.service); await flush();
  const pending = deferred(); env.service.execute = async () => pending.promise;
  env.host.commands.get("dygnosis.recheckProject")(); await flush();
  env.service.client.notify(facts([root("checked", { errors: 99 })], { pass_revision: 2 }));
  pending.resolve(facts([root("pending")], { pass_revision: 3 })); await flush();
  assert.match(env.host.items[0].text, /0\/1 finished/); assert.doesNotMatch(env.host.items[0].text, /99/); registration.dispose();
});
test("a buffered old-instance completion cannot replace a restarted instance's status", async () => {
  const env = setup(), registration = registerProjectStatus(env.service); await flush();
  const oldClient = env.service.client, pending = deferred(); env.service.execute = async () => pending.promise;
  env.host.commands.get("dygnosis.recheckProject")(); await flush();
  oldClient.notify(facts([root("checked", { errors: 99 })], { pass_revision: 2 }));
  env.service.client = connection(); ++env.service.currentInstance; env.service.execute = async () => facts([], { pass_revision: 0 }); env.changed.fire(); await flush();
  pending.resolve(facts([root("pending")], { pass_revision: 2 })); await flush();
  assert.match(env.host.items[0].text, /no root models/); assert.doesNotMatch(env.host.items[0].text, /99/); registration.dispose();
});
test("cancel reply defeats a same-pass pre-cancel notification while retaining a newer cancelled report", async () => {
  for (const newerCancelled of [false, true]) {
    const env = setup(), registration = registerProjectStatus(env.service); await flush();
    const pending = deferred(); env.service.execute = async () => pending.promise;
    env.host.commands.get("dygnosis.cancelProject")(); await flush();
    env.service.client.notify(facts([root("checked", { errors: newerCancelled ? 4 : 99 })],
      newerCancelled ? { cancelled: true, complete: false, coverage_complete: false } : {}));
    if (newerCancelled) env.service.client.notify(facts([root("checked", { errors: 99 })]));
    pending.resolve(facts([root("pending")], { cancelled: true })); await flush();
    assert.match(env.host.items[0].text, /cancelled/);
    assert.equal(rows(env)[0].children[0].description, newerCancelled ? "Checked · 4 Errors, 0 Warnings" : "Pending");
    assert.doesNotMatch(env.host.items[0].tooltip, /99/); registration.dispose();
  }
});
test("folder and settings changes reject replies captured under old inputs and clear contributions immediately", async () => {
  for (const change of ["folder", "settings"]) {
    const env = setup({ client: connection() }), first = deferred(), second = deferred();
    env.service.execute = async () => first.promise;
    const registration = registerProjectStatus(env.service); await flush();
    env.service.execute = async () => second.promise;
    if (change === "folder") { env.host.folders = [folder(other, "other")]; env.host.folderChanged.fire(); }
    else { env.configure("projectExcludePaths", ["generated/**"]); }
    assert.equal(rows(env).length, 0); await flush();
    first.resolve(facts([root("checked", { errors: 99 })])); await flush(); assert.doesNotMatch(env.host.items[0].text, /99/);
    second.resolve(facts([root("checked", { root_uri: `${other}/main.mod`, errors: 2 })], { pass_revision: 2 })); await flush();
    assert.match(env.host.items[0].text, /2 Errors/); registration.dispose();
  }
});
test("disabling queued work clears the project contribution and cannot be undone by a late enabled reply", async () => {
  const env = setup(), pending = deferred(); const registration = registerProjectStatus(env.service); await flush();
  env.service.execute = async () => pending.promise; env.host.commands.get("dygnosis.recheckProject")(); await flush();
  env.configure("projectDiagnostics", false); assert.match(env.host.items[0].text, /off/); assert.equal(rows(env).length, 0);
  assert.equal(liveWatchers(env).length, 0);
  pending.resolve(facts([root("checked", { errors: 99 })], { pass_revision: 2 })); await flush();
  assert.match(env.host.items[0].text, /off/); assert.doesNotMatch(env.host.items[0].text, /99/); registration.dispose();
});
test("server replacement drops old subscriptions, watchers and responses, then queries the new instance", async () => {
  const first = connection(), pending = deferred(), env = setup({ client: first }); env.service.execute = async () => pending.promise;
  const registration = registerProjectStatus(env.service); await flush();
  const second = connection(); env.service.client = second; ++env.service.currentInstance; env.service.execute = async () => facts([], { pass_revision: 0 }); env.changed.fire(); await flush();
  assert.equal(first.subscriptions.get(capability.status_notification).listeners.size, 0);
  assert.equal(liveWatchers(env, "**/*.mod").length, 1); assert.match(env.host.items[0].text, /no root models/);
  pending.resolve(facts([root("checked", { errors: 99 })])); first.notify(facts([root("checked", { errors: 99 })])); await flush();
  assert.doesNotMatch(env.host.items[0].text, /99/); assert.ok(second.sent.some(value => value.name === capability.active_model_notification));
  second.running = false; env.changed.fire(); assert.equal(second.subscriptions.get(capability.status_notification).listeners.size, 0); assert.equal(liveWatchers(env).length, 0);
  registration.dispose();
});
test("Recheck and Cancel use advertised commands and preserve their different lifetimes", async () => {
  const env = setup(); const registration = registerProjectStatus(env.service); await flush();
  env.service.status = facts([root("pending")], { pass_revision: 2 }); env.host.commands.get("dygnosis.recheckProject")(); await flush();
  assert.equal(env.service.queries.at(-1).command, capability.recheck_command); assert.match(env.host.items[0].text, /0\/1 finished/);
  env.service.status = facts([root("pending")], { pass_revision: 2, cancelled: true }); env.host.commands.get("dygnosis.cancelProject")(); await flush();
  assert.equal(env.service.queries.at(-1).command, capability.cancel_command); assert.match(env.host.items[0].text, /cancelled/);
  assert.equal(env.host.settings.projectDiagnostics, undefined); assert.match(env.host.items[0].tooltip, /file edit\/change or Recheck/);
  env.service.client.notify(facts([root()], { pass_revision: 3 })); assert.match(env.host.items[0].text, /1\/1 checked/);
  registration.dispose();
});
test("rapid overlay edits coalesce a status query and close refreshes saved coverage", async () => {
  const env = setup(), registration = registerProjectStatus(env.service); await flush();
  const doc = document(), before = env.service.queries.length;
  env.service.status = facts([root("pending")], { pass_revision: 2 });
  for (let index = 0; index < 4; ++index) { ++doc.version; env.host.edited.fire({ document: doc }); }
  assert.match(env.host.items[0].text, /updating/); assert.equal(env.service.queries.length, before);
  await new Promise(resolve => setTimeout(resolve, 90)); await flush();
  assert.equal(env.service.queries.length, before + 1); assert.match(env.host.items[0].text, /0\/1 finished/);
  env.service.status = facts([root("checked", { errors: 1 })], { pass_revision: 3 }); doc.isClosed = true; env.host.closed.fire(doc);
  assert.match(env.host.items[0].text, /updating/); await new Promise(resolve => setTimeout(resolve, 90)); await flush();
  assert.match(env.host.items[0].text, /1 Error/); registration.dispose();
});
test("per-folder exclusion controls write only the chosen resource and reset its override", async () => {
  const env = setup({ folders: [folder(), folder(other, "other")] }); env.host.resources.set(other, { projectExcludePaths: ["old/**"] });
  const registration = registerProjectStatus(env.service); await flush();
  env.host.choices.push("other", "add"); env.host.inputValues.push("generated/**"); await env.host.commands.get("dygnosis.configureProjectExclusions")();
  assert.deepEqual(env.host.updates[0].value, ["old/**", "generated/**"]); assert.equal(env.host.updates[0].target, vscode.ConfigurationTarget.WorkspaceFolder);
  assert.equal(env.host.updates[0].resource.toString(), other);
  env.host.choices.push("reset"); await env.host.commands.get("dygnosis.configureProjectExclusions")(rows(env)[1]); assert.equal(env.host.updates[1].value, undefined);
  env.host.choices.push("remove", "old/**"); await env.host.commands.get("dygnosis.configureProjectExclusions")(rows(env)[1]); assert.deepEqual(env.host.updates[2].value, []);
  registration.dispose();
});
test("malformed notification clears stale coverage, and disposal prevents late updates and releases every listener", async () => {
  const env = setup(); const registration = registerProjectStatus(env.service); await flush();
  env.service.client.notify({ ...facts(), schema_version: 9 }); assert.match(env.host.items[0].text, /unavailable/); assert.equal(rows(env).length, 0);
  const pending = deferred(); env.service.execute = async () => pending.promise; env.host.commands.get("dygnosis.recheckProject")(); await flush();
  registration.dispose(); pending.resolve(facts([root("checked", { errors: 99 })])); await flush();
  assert.equal(env.host.items[0].visible, false); assert.equal(env.host.items[0].disposed, true); assert.equal(env.host.views[0].disposed, true);
  assert.equal(liveWatchers(env).length, 0); assert.equal(env.host.commands.size, 0);
  for (const emitter of [env.changed, env.host.active, env.host.folderChanged, env.host.configured, env.host.edited, env.host.closed]) assert.equal(emitter.listeners.size, 0);
  assert.equal(env.service.client.subscriptions.get(capability.status_notification).listeners.size, 0);
});
