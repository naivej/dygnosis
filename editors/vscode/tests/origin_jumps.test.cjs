const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");
const path = require("node:path");
const { minimatch } = require("minimatch");

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
  cancelled = new Emitter();
  token = { isCancellationRequested: false, onCancellationRequested: this.cancelled.event };
  cancel() { if (!this.token.isCancellationRequested) { this.token.isCancellationRequested = true; this.cancelled.fire(); } }
  dispose() { this.disposed = true; this.cancelled.dispose(); }
}
class Range {
  constructor(line, character, endLine = line, endCharacter = character) {
    this.start = { line, character }; this.end = { line: endLine, character: endCharacter };
  }
}
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname),
    query: parsed.search, fragment: parsed.hash, toString: () => value };
}
function doc(value, text = "model;\ny=1;\nend;", version = 1) {
  return { uri: uri(value), text, version, languageId: "dynare", isClosed: false,
    getText() { return this.text; }, get lineCount() { return this.text.split(/\r\n|\r|\n/).length; },
    lineAt(line) { return { text: this.text.split(/\r\n|\r|\n/)[line] }; } };
}
function deferred() { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; }
function matches(pattern, value) {
  const relative = typeof pattern === "string" ? value.fsPath.replace(/^\/+/, "") : path.relative(pattern.base.fsPath, value.fsPath).replaceAll("\\", "/");
  return minimatch(relative, typeof pattern === "string" ? pattern : pattern.pattern, { dot: true });
}
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
const vscode = {
  Disposable, EventEmitter: Emitter, CancellationTokenSource, Range, TreeItem: class {}, ViewColumn: { Beside: 2 },
  RelativePattern: class { constructor(base, pattern) { this.base = base; this.pattern = pattern; } },
  Uri: { parse: uri, file: value => uri(`file://${value.startsWith("/") ? "" : "/"}${value.replaceAll("\\", "/")}`),
    from: value => uri(`${value.scheme}:${value.path}`) },
  commands: {
    registerCommand: (id, action) => { host.commands.set(id, action); return new Disposable(() => host.commands.delete(id)); },
    executeCommand: async (id, ...args) => {
      if (id === "setContext") { host.contexts.set(args[0], args[1]); return; }
      return host.commands.get(id)?.(...args);
    },
  },
  languages: { setTextDocumentLanguage: async (document, language) => { document.languageId = language; return document; } },
  window: {
    get activeTextEditor() { return host.editor; },
    get visibleTextEditors() { return host.editor ? [host.editor] : []; },
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
    onDidChangeTextEditorSelection: listener => host.selection.event(listener),
    showQuickPick: async (items, options) => { host.picks.push({ items, options }); return host.pick ? host.pick(items) : items[0]; },
    showInformationMessage: async message => { host.messages.push(message); },
    createOutputChannel: () => ({ appendLine: message => host.clientLogs.push(message), append() {}, show() {}, dispose() {} }),
    showErrorMessage: async message => { host.messages.push(message); },
    showTextDocument: async (document, options) => {
      host.shown.push({ document, options }); host.editor = { document, selection: new Range(1, 0) };
      host.active.fire(host.editor); return host.editor;
    },
  },
  workspace: {
    get textDocuments() { return [...host.documents.values()]; },
    get workspaceFolders() { return []; },
    onDidOpenTextDocument: listener => host.opened.event(listener),
    onDidChangeWorkspaceFolders: listener => host.foldersChanged.event(listener),
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
    getConfiguration: () => ({ get: (key, fallback) => Object.hasOwn(host.settings, key) ? host.settings[key] : fallback,
      inspect: key => ({ globalValue: host.settings[key] }) }),
    asRelativePath: value => value.path.split("/").at(-1),
    registerTextDocumentContentProvider: (scheme, provider) => {
      host.providers.set(scheme, provider);
      const changed = provider.onDidChange?.(value => {
        const document = host.documents.get(value.toString());
        if (document) { document.text = provider.provideTextDocumentContent(value); ++document.version; host.edited.fire({ document, contentChanges: [{ text: document.text }] }); }
      });
      return new Disposable(() => { changed?.dispose(); host.providers.delete(scheme); });
    },
    createFileSystemWatcher: pattern => {
      const create = new Emitter(), change = new Emitter(), remove = new Emitter();
      const subscribe = event => listener => event(value => { if (matches(pattern, value)) listener(value); });
      const watcher = { pattern, create, change, remove, onDidCreate: subscribe(create.event), onDidChange: subscribe(change.event), onDidDelete: subscribe(remove.event),
        dispose() { this.disposed = true; create.dispose(); change.dispose(); remove.dispose(); } };
      host.watchers.push(watcher); return watcher;
    },
    openTextDocument: async value => {
      host.loads.push(value.toString());
      if (host.load) return host.load(value);
      let document = host.documents.get(value.toString());
      if (!document) {
        const provider = host.providers.get(value.scheme);
        document = doc(value.toString(), provider?.provideTextDocumentContent(value) ?? "model;\ny=1;\nend;");
        host.documents.set(value.toString(), document);
      }
      return document;
    },
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (id === "vscode-languageclient/node") return { DocumentSymbolRequest: { method: "textDocument/documentSymbol" }, ExecuteCommandRequest: { type: "workspace/executeCommand" }, State: { Stopped: 1 } };
  if (id === "./client" && /(?:model_view|preview)\.js$/.test(args[0]?.filename ?? "")) return {
    isAnalysisDocument: document => document.languageId === "dynare" && ["file", "untitled"].includes(document.uri.scheme),
  };
  return originalLoad.call(this, id, ...args);
};
const { registerEffectivePreview } = require("../out/preview");
const { registerOriginJumps, parsePreviewNavigation, previewRowAt, writtenSourcePicks, macroOriginPicks, originJumpContributions } = require("../out/origin_jumps");
const { DygnosisClient } = require("../out/client");
Module._load = originalLoad;

const main = "file:///project/main.mod";
function target(value = main, range = new Range(1, 0, 1, 3), version = 1) { return { uri: value, range, document_version: version }; }
function row(extra = {}) {
  return { id: "e1", statement_id: "s1", effective_range: new Range(1, 0, 1, 5), written_locations: [target()], macro_frames: [],
    kind: "equation", active: true, number: 1, scope: "aggregate", dimension: null, ...extra };
}
function payload(extra = {}) {
  return { effective_text: "model;\ny = 1;\nend;", root_uri: main, revision: "current", document_version: 1,
    complete: true, navigation_schema_version: 1, navigation: [row()], dependency_candidates: [main], ...extra };
}
function setup(options = {}) {
  host = { commands: new Map(), contexts: new Map(), providers: new Map(), documents: new Map(), watchers: [], shown: [], loads: [],
    picks: [], messages: [], clientLogs: [], settings: {}, active: new Emitter(), selection: new Emitter(), edited: new Emitter(), closed: new Emitter(), configured: new Emitter(),
    opened: new Emitter(), foldersChanged: new Emitter() };
  const rootDocument = doc(main); host.documents.set(main, rootDocument); host.editor = { document: rootDocument, selection: new Range(1, 0) };
  const changed = new Emitter(), presentation = new Emitter();
  const service = { currentInstance: 1, changed, presentation, onDidChange: presentation.event, onDidInvalidate: changed.event,
    logged: [], requests: [], validations: [], jumps: [],
    client: { initializeResult: { capabilities: { experimental: { dygnosis: { effectivePreview: {
      command: "dynare/showEffectiveModel", navigation_schema_version: 1, dependency_candidates: true,
    } } } } } },
    log: message => service.logged.push(message), failure: async message => service.logged.push(message),
    ensureStarted: async () => {},
    rootForDocument: async document => service.root ?? document.uri,
    value: payload(),
    execute: async (command, args, token) => {
      service.requests.push({ command, args, token });
      return service.executeHook ? service.executeHook(command, args, token) : JSON.parse(JSON.stringify(service.value));
    },
    revalidate: async (root, revision, instance, resource, token) => {
      service.validations.push({ root: root.toString(), revision, instance, resource: resource.toString(), token });
      if (service.validateHook) return service.validateHook(root, revision, instance, resource, token);
      return instance === service.currentInstance && service.value.revision === revision
        ? { complete: service.value.complete, revision, document_version: service.value.document_version } : undefined;
    },
    openLocation: async (location, root, guard, options) => {
      const document = await vscode.workspace.openTextDocument(uri(location.uri));
      const decision = await guard(document);
      if (typeof decision === "boolean" ? !decision : decision.isCurrent() !== true) return;
      service.jumps.push({ location, root: root.toString(), options }); await vscode.window.showTextDocument(document, { ...options, selection: location.range });
    },
  };
  Object.assign(service, options);
  const previews = registerEffectivePreview(service), registration = registerOriginJumps(service, previews);
  const run = id => host.commands.get(`dygnosis.${id}`)();
  const show = async () => { await run("showEffectiveModel"); await flush(); return previews.sessions().at(-1); };
  const activate = session => { host.editor = { document: session.document, selection: new Range(1, 0) }; host.active.fire(host.editor); };
  const edit = (document = rootDocument) => { ++document.version; host.edited.fire({ document, contentChanges: [{ text: "changed" }] }); };
  const close = session => { session.document.isClosed = true; host.documents.delete(session.uri.toString()); host.closed.fire(session.document); };
  return { host, rootDocument, service, previews, registration, run, show, activate, edit, close };
}

async function realClientSetup() {
  const base = setup(); base.registration.dispose(); base.previews.dispose();
  const engine = { value: payload(), infos: [], hold: false };
  engine.snapshot = () => ({ schema_version: 1, root_uri: main, document_uri: main, document_version: base.rootDocument.version,
    revision: engine.value.revision, complete: true, owner_roots: [main], statements: [], declarations: [], equations: [], related_files: [],
    block_categories: [], first_model_anchor: null, dependency_candidates: engine.value.dependency_candidates,
    n_endogenous: 1, n_exogenous: 0, n_parameters: 0, n_equations: 1, endogenous: ["y"], exogenous: [], parameters: [],
    static: ["y"], predetermined: [], forward_looking: [], mixed: [], heterogeneity_dimensions: [] });
  const service = new DygnosisClient({}, {
    resolve: async () => ({ path: "test engine", version: "one", override: true }),
    create: (_binary, _output, _middleware, _settings, capture) => {
      const client = {
        initializeResult: { capabilities: { documentSymbolProvider: true, executeCommandProvider: { commands: ["dynare/modelInfo", "dynare/showEffectiveModel"] },
          experimental: { dygnosis: { modelInfo: { schema_version: 1 }, effectivePreview: { command: "dynare/showEffectiveModel", navigation_schema_version: 1, dependency_candidates: true } } } } },
        sendNotification: async () => {}, onNotification: () => new Disposable(), onDidChangeState: () => new Disposable(),
        getFeature: () => ({ clear() {}, unregister() {}, register() {} }),
        sendRequest: (_method, params, token) => {
          if (params.command === "dynare/showEffectiveModel") return Promise.resolve(JSON.parse(JSON.stringify(engine.value)));
          const reply = deferred(), call = { token, reply }; engine.infos.push(call);
          if (engine.infoHook?.(call)) return reply.promise;
          if (!engine.hold) reply.resolve(engine.snapshot());
          return reply.promise;
        },
      };
      return { client, start: async () => { capture({ workspaceFolders: [] }); }, dispose: async () => {} };
    },
  });
  await service.ensureStarted();
  const previews = registerEffectivePreview(service), registration = registerOriginJumps(service, previews);
  const show = async () => { base.host.editor = { document: base.rootDocument, selection: new Range(1, 0) }; await base.run("showEffectiveModel"); await flush(); return previews.sessions().at(-1); };
  return { ...base, engine, service, previews, registration, show,
    dispose: async () => { registration.dispose(); previews.dispose(); await service.shutdown(); } };
}

test("navigation schema checks exact UTF-16 and CRLF bounds without recovering rows from text", () => {
  const value = payload({ effective_text: "model;\r\ny = '😀';\r\nend;", navigation: [row({ effective_range: new Range(1, 0, 1, 8) })] });
  assert.equal(parsePreviewNavigation(value, uri(main)).navigation.length, 1);
  for (const changes of [
    { navigation_schema_version: 2 }, { root_uri: "file:///wrong.mod" }, { document_version: -1 }, { revision: "" },
    { complete: false }, { dependency_candidates: ["https://wrong/file.mod"] },
    { navigation: [row(), row()] }, { navigation: [row({ effective_range: new Range(1, 0, 1, 6) })] },
    { navigation: [row({ effective_range: new Range(1, 0, 4, 0) })] },
    { navigation: [row({ written_locations: [target("https://wrong/model.mod")] })] },
    { navigation: [row({ written_locations: [target(main, new Range(1, 4, 1, 1))] })] },
  ]) assert.throws(() => parsePreviewNavigation({ ...value, ...changes }, uri(main)), /navigation/);
  assert.equal(parsePreviewNavigation(payload({ complete: false, navigation: [] }), uri(main)).complete, false);
});

test("cursor and selections respect half-open ranges and reject ambiguous rows", () => {
  const rows = [row(), row({ id: "local", number: null, kind: "local", effective_range: new Range(2, 0, 2, 10) })];
  assert.equal(previewRowAt(rows, new Range(1, 0)).id, "e1");
  assert.equal(previewRowAt(rows, new Range(1, 5)), undefined);
  assert.equal(previewRowAt(rows, new Range(0, 0)), undefined);
  assert.equal(previewRowAt(rows, new Range(2, 2, 2, 4)).id, "local");
  assert.equal(previewRowAt(rows, new Range(1, 2, 2, 4)), undefined);
  assert.equal(previewRowAt([row({ effective_range: new Range(1, 0, 3, 5) })], new Range(2, 9)).id, "e1");
});

test("source and macro pick labels retain scope, occurrence, kind and loop binding", () => {
  const value = row({ scope: "heterogeneous", dimension: "households", macro_frames: [
    { kind: "for", variable: "country", value: '"US"', directive_locations: [target(main, new Range(0, 0, 0, 9))], body_locations: [target("file:///project/body.inc", new Range(2, 0, 2, 3), null)] },
    { kind: "if", variable: null, value: null, directive_locations: [target()], body_locations: [] },
  ] });
  assert.match(writtenSourcePicks(value)[0].description, /Dimension households.*Occurrence e1/);
  const picks = macroOriginPicks(value);
  assert.deepEqual(picks.map(pick => pick.label), ['for · country="US" · directive', 'for · country="US" · body', "if · directive"]);
  assert.match(picks[1].description, /body\.inc:3/);
  assert.equal(picks[1].frameIndex, 0); assert.equal(picks[1].site, "body");
});

test("menus and native keys are preview-scoped and toolbar placements have separate controls", () => {
  assert.deepEqual(originJumpContributions.commands.map(command => command.command), ["dygnosis.goToWrittenSource", "dygnosis.showMacroOrigins", "dygnosis.refreshEffectiveModel"]);
  for (const items of Object.values(originJumpContributions.menus)) for (const item of items) assert.match(item.when, /resourceScheme == dygnosis-effective/);
  for (const binding of originJumpContributions.keybindings) assert.match(binding.when, /dygnosis-effective.*editorTextFocus/);
  assert.match(originJumpContributions.menus["editor/title"][0].when, /previewToolbarActions/);
  assert.match(originJumpContributions.menus["editor/context"][0].when, /previewContextActions/);
});

test("one verified source opens beside the preview with its stored root", async () => {
  const env = setup(), session = await env.show();
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  await env.run("goToWrittenSource");
  assert.equal(env.service.jumps.length, 1);
  assert.equal(env.service.jumps[0].root, main);
  assert.deepEqual(env.service.jumps[0].options, { viewColumn: 2 });
  assert.equal(env.host.picks.length, 0);
  assert.equal(session.uri.scheme, "dygnosis-effective");
  assert.equal(session.document.languageId, "dynare");
  assert.equal(env.previews.sessions().length, 1);
  env.registration.dispose(); env.previews.dispose();
});

test("included and split rows offer every verified written portion", async () => {
  const env = setup();
  env.service.value = payload({ navigation: [row({ written_locations: [target(), target("file:///project/body.inc", new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, "file:///project/body.inc"] });
  const session = await env.show();
  env.host.pick = items => items[1];
  await env.run("goToWrittenSource");
  assert.equal(env.host.picks[0].items.length, 2);
  assert.equal(env.service.jumps[0].location.uri, "file:///project/body.inc");
  assert.equal(env.service.jumps[0].root, session.root.toString());
  env.registration.dispose(); env.previews.dispose();
});

test("macro origin command always picks directive and clipped body locations", async () => {
  const env = setup();
  env.service.value.navigation[0].macro_frames = [
    { kind: "for", variable: "i", value: "2", directive_locations: [target(main, new Range(0, 0, 0, 6))], body_locations: [target()] },
    { kind: "if", variable: null, value: null, directive_locations: [target(main, new Range(0, 0, 0, 3))], body_locations: [] },
  ];
  await env.show(); env.host.pick = items => items[1];
  await env.run("showMacroOrigins");
  assert.equal(env.host.picks[0].items.length, 3);
  assert.equal(env.service.jumps.length, 1);
  assert.match(env.host.picks[0].items[0].label, /i=2/);
  env.registration.dispose(); env.previews.dispose();
});

test("unmapped text and ambiguous selections do no request or source load", async () => {
  const env = setup();
  env.service.value.navigation.push(row({ id: "local", kind: "local", number: null, effective_range: new Range(2, 0, 2, 3) }));
  await env.show(); const calls = env.service.requests.length;
  for (const selection of [new Range(0, 1), new Range(1, 5), new Range(1, 0, 2, 2)]) {
    env.host.editor.selection = selection; env.host.selection.fire(); await env.run("goToWrittenSource");
  }
  assert.equal(env.service.requests.length, calls); assert.equal(env.service.jumps.length, 0);
  env.registration.dispose(); env.previews.dispose();
});

test("root edits disable jumps until refresh atomically replaces text and navigation", async () => {
  const env = setup(), session = await env.show();
  env.edit(); env.service.value = payload({ revision: "edited", document_version: 2, effective_text: "model;\ny = 22;\nend;", navigation: [row({ id: "changed", effective_range: new Range(1, 0, 1, 6), written_locations: [target(main, new Range(1, 0, 1, 4), 2)] })] });
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0);
  await env.run("refreshEffectiveModel");
  assert.equal(session.text, env.service.value.effective_text);
  assert.equal(session.result.navigation[0].id, "changed");
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 1);
  env.registration.dispose(); env.previews.dispose();
});

test("dependency candidates watch missing arbitrary-extension files and invalidate on create/change/delete", async () => {
  for (const event of ["create", "change", "remove"]) {
    const env = setup(); env.service.value.dependency_candidates.push("file:///project/missing.macro"); await env.show();
    assert.ok(env.host.watchers.every(watcher => watcher.pattern.pattern === "*"));
    for (const watcher of env.host.watchers) watcher[event].fire(uri("file:///project/missing.macro"));
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0);
    env.registration.dispose(); env.previews.dispose(); assert.ok(env.host.watchers.every(watcher => watcher.disposed));
  }
});

test("settings and server invalidation withhold origins and obey action placement controls", async () => {
  const env = setup(); await env.show();
  env.host.settings.editorActions = [];
  env.host.configured.fire({ affectsConfiguration: () => true });
  assert.equal(env.host.contexts.get("dygnosis.previewToolbarActions"), false);
  assert.equal(env.host.contexts.get("dygnosis.previewContextActions"), false);
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  await env.run("refreshEffectiveModel");
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  ++env.service.currentInstance; env.service.changed.fire({ reason: "input" });
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  await env.run("refreshEffectiveModel");
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  env.registration.dispose(); env.previews.dispose();
});

test("several previews retain independent roots and refresh an unopened root", async () => {
  const env = setup(), first = await env.show();
  const other = "file:///project/other.dyn", otherDocument = doc(other);
  env.host.documents.set(other, otherDocument); env.host.editor = { document: otherDocument, selection: new Range(1, 0) };
  env.service.value = payload({ root_uri: other, navigation: [row({ written_locations: [target(other)] })], dependency_candidates: [other] });
  const second = await env.show(); assert.equal(env.previews.sessions().length, 2);
  env.host.documents.delete(main); env.rootDocument.isClosed = true;
  env.service.value = payload({ document_version: null, navigation: [row({ written_locations: [target(main, new Range(1, 0, 1, 3), null)] })] });
  env.activate(first); await env.run("refreshEffectiveModel");
  assert.equal(env.service.requests.at(-1).args[0].root_uri, main);
  assert.equal(first.result.document_version, null); assert.equal(second.result.root_uri, other);
  env.host.load = async value => {
    const loaded = doc(value.toString()); env.host.documents.set(value.toString(), loaded);
    env.service.value.document_version = loaded.version; env.service.value.navigation[0].written_locations[0].document_version = loaded.version;
    return loaded;
  };
  await env.run("goToWrittenSource"); assert.equal(env.service.jumps[0].root, main);
  env.registration.dispose(); env.previews.dispose();
});

test("older override preserves basic preview and refresh while origins remain unavailable", async () => {
  const env = setup(); delete env.service.client.initializeResult.capabilities.experimental;
  env.service.value = { effective_text: "model;\ny = 1;\nend;" };
  const session = await env.show(); assert.ok(session.text.includes("y = 1"));
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0);
  env.service.value = { effective_text: "model;\ny = 2;\nend;" }; await env.run("refreshEffectiveModel");
  assert.ok(session.text.includes("y = 2")); assert.equal(env.service.logged.length, 0);
  env.registration.dispose(); env.previews.dispose();
});

test("incomplete text keeps its label and supplies no origins", async () => {
  const env = setup(); env.service.value = payload({ complete: false, status: "incomplete", navigation: [] });
  const session = await env.show(); assert.match(session.text, /^\/\/ INCOMPLETE EXPANSION/);
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  await env.run("showMacroOrigins"); assert.equal(env.host.picks.length, 0);
  env.registration.dispose(); env.previews.dispose();
});

test("unsupported advertised schema reports recovery without guessed targets", async () => {
  const env = setup(); env.service.value.navigation_schema_version = 2; await env.show();
  assert.match(env.service.logged[0], /Update dynare.serverPath/);
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  env.registration.dispose(); env.previews.dispose();
});

test("picker cancellation, cursor movement, and input invalidation cannot open old targets", async () => {
  for (const kind of ["cancel", "cursor", "edit", "root"]) {
    const env = setup(); env.service.value.navigation[0].written_locations.push(target(main, new Range(2, 0, 2, 3))); await env.show();
    env.host.pick = items => {
      if (kind === "cancel") return undefined;
      if (kind === "cursor") env.host.editor.selection = new Range(0, 0);
      if (kind === "edit") env.edit();
      if (kind === "root") env.host.editor = { document: env.rootDocument, selection: new Range(1, 0) };
      return items[0];
    };
    await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0, kind);
    env.registration.dispose(); env.previews.dispose();
  }
});

test("chosen target disappearing after picker disables the stale snapshot", async () => {
  const env = setup(); env.service.value.navigation[0].written_locations.push(target(main, new Range(2, 0, 2, 3))); await env.show();
  env.host.pick = items => { env.service.value.navigation[0].written_locations = [items[1].target]; return items[0]; };
  await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0);
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  assert.match(env.host.messages[0], /Refresh effective model/);
  env.registration.dispose(); env.previews.dispose();
});

test("source load validates URI identity and live source version before owner or editor change", async () => {
  for (const kind of ["wrong-uri", "version", "edited-model", "closed", "missing", "range", "unobserved-dirty"]) {
    const env = setup();
    if (kind === "unobserved-dirty") env.service.value.navigation[0].written_locations[0].document_version = null;
    await env.show();
    env.host.load = async value => {
      if (kind === "missing") throw new Error("file missing");
      const source = doc(kind === "wrong-uri" ? "file:///other/model.mod" : value.toString());
      if (kind === "version") source.version = 2;
      if (kind === "edited-model") env.service.value.revision = "changed";
      if (kind === "closed") source.isClosed = true;
      if (kind === "range") source.text = "";
      if (kind === "unobserved-dirty") source.isDirty = true;
      return source;
    };
    await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0, kind);
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false, `${kind} must disable its current snapshot`);
    env.registration.dispose(); env.previews.dispose();
  }
});

test("native Windows source identity survives changed URI spelling after loading", async () => {
  const env = setup(), source = "file:///C:/Models/Body.inc";
  env.service.value.navigation[0].written_locations = [target(source, new Range(0, 0, 0, 3), null)]; await env.show();
  env.host.load = async () => doc("file:///c:/models/body.inc");
  await env.run("goToWrittenSource");
  assert.equal(env.service.jumps.length, process.platform === "win32" ? 1 : 0);
  env.registration.dispose(); env.previews.dispose();
});

test("superseded refresh cancels old result and publishes one text/navigation pair", async () => {
  const env = setup(), session = await env.show(), first = deferred(), second = deferred();
  const results = [first, second]; env.service.executeHook = () => results.shift().promise;
  const oldRefresh = env.run("refreshEffectiveModel"), oldToken = env.service.requests.at(-1).token;
  const latestRefresh = env.run("refreshEffectiveModel"); assert.equal(oldToken.isCancellationRequested, true);
  second.resolve(payload({ effective_text: "model;\ny = 2;\nend;", navigation: [row({ id: "second" })] })); await latestRefresh;
  first.resolve(payload({ effective_text: "model;\ny = 9;\nend;", navigation: [row({ id: "first" })] })); await oldRefresh;
  assert.equal(session.result.navigation[0].id, "second"); assert.match(session.text, /y = 2/);
  env.registration.dispose(); env.previews.dispose();
});

test("refresh rejects changed source version, dependency/settings input, and engine instance mid-request", async () => {
  for (const kind of ["version", "edit", "settings", "restart"]) {
    const env = setup(), session = await env.show(), pending = deferred(); env.service.executeHook = () => pending.promise;
    const refresh = env.run("refreshEffectiveModel");
    if (kind === "version") ++env.rootDocument.version;
    if (kind === "edit") env.edit();
    if (kind === "settings") env.host.configured.fire({ affectsConfiguration: () => true });
    if (kind === "restart") ++env.service.currentInstance;
    pending.resolve(payload({ effective_text: "model;\ny = 8;\nend;" })); await refresh;
    assert.match(session.text, /y = 1/, kind); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    env.registration.dispose(); env.previews.dispose();
  }
});

test("closing a preview cancels work and disposes its subscriptions and dependency watchers", async () => {
  const env = setup(), session = await env.show(), pending = deferred(); env.service.executeHook = () => pending.promise;
  const refreshing = env.run("refreshEffectiveModel"), token = env.service.requests.at(-1).token;
  env.close(session);
  assert.equal(token.isCancellationRequested, true); assert.equal(env.service.changed.listeners.size, 0);
  assert.ok(env.host.watchers.every(watcher => watcher.disposed)); assert.equal(env.previews.sessions().length, 0);
  pending.resolve(payload({ effective_text: "late" })); await refreshing;
  assert.equal(env.host.contexts.get("dygnosis.effectivePreview"), false);
  env.registration.dispose(); env.previews.dispose(); assert.equal(env.host.providers.size, 0);
});

test("commands outside a registered virtual preview do no model work", async () => {
  const env = setup(); await env.show(); env.host.editor = { document: env.rootDocument, selection: new Range(1, 0) }; env.host.active.fire();
  const calls = env.service.requests.length;
  for (const id of ["goToWrittenSource", "showMacroOrigins", "refreshEffectiveModel"]) await env.run(id);
  assert.equal(env.service.requests.length, calls); assert.equal(env.host.contexts.get("dygnosis.effectivePreview"), false);
  env.registration.dispose(); env.previews.dispose();
});

test("owner presentation updates do not stale previews and root invalidations stay scoped", async () => {
  const env = setup(), first = await env.show();
  env.service.presentation.fire(); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  const other = "file:///project/second.mod", otherDocument = doc(other);
  env.host.documents.set(other, otherDocument); env.host.editor = { document: otherDocument, selection: new Range(1, 0) };
  env.service.value = payload({ root_uri: other, navigation: [row({ written_locations: [target(other)] })], dependency_candidates: [other] });
  const second = await env.show(); env.service.changed.fire({ root: other, reason: "input" });
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  env.activate(first); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  env.activate(second); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  env.registration.dispose(); env.previews.dispose();
});

test("initial preview discards old engine and native document contexts before exposing a session", async () => {
  for (const kind of ["restart", "edit", "owner", "active"]) {
    const env = setup(), pending = deferred(); env.service.executeHook = () => pending.promise;
    const showing = env.show(); await flush();
    if (kind === "restart") ++env.service.currentInstance;
    if (kind === "edit") env.edit();
    if (kind === "owner") env.service.root = uri("file:///other/root.mod");
    if (kind === "active") env.host.editor = undefined;
    pending.resolve(payload()); await showing;
    assert.equal(env.previews.sessions().length, 0, kind);
    assert.equal(env.host.shown.length, 0, kind);
    env.registration.dispose(); env.previews.dispose();
  }
});

test("macro frame binding changes after a pick cannot use its formerly labelled target", async () => {
  const env = setup(); env.service.value.navigation[0].macro_frames = [
    { kind: "for", variable: "i", value: "1", directive_locations: [target()], body_locations: [] },
  ];
  await env.show(); env.host.pick = items => { env.service.value.navigation[0].macro_frames[0].value = "2"; return items[0]; };
  await env.run("showMacroOrigins"); assert.equal(env.service.jumps.length, 0);
  assert.equal(env.host.contexts.get("dygnosis.previewMacroOrigins"), false);
  env.registration.dispose(); env.previews.dispose();
});

test("source edits during post-loader revalidation are rejected by the loaded document guard", async () => {
  const env = setup(); await env.show();
  let loaded;
  env.host.load = async value => { loaded = doc(value.toString()); return loaded; };
  let calls = 0;
  env.service.executeHook = () => {
    ++calls;
    if (calls === 3) ++loaded.version;
    return JSON.parse(JSON.stringify(env.service.value));
  };
  await env.run("goToWrittenSource"); assert.equal(env.service.jumps.length, 0);
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  env.registration.dispose(); env.previews.dispose();
});

test("a superseded source load cannot stale a newer refresh for valid or invalid loaded documents", async () => {
  for (const kind of ["valid", "closed", "wrong-uri", "version", "range"]) {
    const env = setup(), session = await env.show(), held = deferred();
    env.host.load = () => held.promise;
    const oldNavigation = env.run("goToWrittenSource"); await flush();
    assert.equal(env.host.loads.length, 2, "the source load must be pending after both navigation checks");
    env.service.value = payload({ revision: "newer", effective_text: "model;\ny = 2;\nend;", navigation: [row({ id: "newer-row" })] });
    await env.run("refreshEffectiveModel");
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
    const loaded = doc(kind === "wrong-uri" ? "file:///wrong/model.mod" : main);
    if (kind === "closed") loaded.isClosed = true;
    if (kind === "version") ++loaded.version;
    if (kind === "range") loaded.text = "";
    held.resolve(loaded); await oldNavigation;
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true, kind);
    assert.equal(session.result.navigation[0].id, "newer-row"); assert.match(session.text, /y = 2/);
    assert.equal(env.service.jumps.length, 0); assert.equal(env.host.messages.length, 0);
    env.registration.dispose(); env.previews.dispose();
  }
});

test("a superseded post-loader validation cannot disable newly committed navigation", async () => {
  const env = setup(), session = await env.show(), held = deferred(), original = payload();
  let requests = 0;
  env.service.executeHook = () => ++requests === 3 ? held.promise : JSON.parse(JSON.stringify(env.service.value));
  const oldNavigation = env.run("goToWrittenSource"); await flush();
  assert.equal(requests, 3, "the post-loader validation must be pending");
  env.service.value = payload({ revision: "newer", effective_text: "model;\ny = 2;\nend;", navigation: [row({ id: "newer-row" })] });
  await env.run("refreshEffectiveModel");
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  held.resolve(original); await oldNavigation;
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  assert.equal(session.result.navigation[0].id, "newer-row"); assert.match(session.text, /y = 2/);
  assert.equal(env.service.jumps.length, 0); assert.equal(env.host.messages.length, 0);
  env.registration.dispose(); env.previews.dispose();
});

test("preview watcher patterns actually match literal bracket/brace candidates and filter unrelated names", async () => {
  for (const filename of ["[part].macro", "{part,other}.macro"]) for (const event of ["create", "change", "remove"]) {
    const env = setup(), candidate = uri(`file:///project/${filename}`);
    env.service.value.dependency_candidates.push(candidate.toString()); await env.show();
    assert.ok(env.host.watchers.every(watcher => matches(watcher.pattern, candidate)));
    for (const watcher of env.host.watchers) watcher[event].fire(uri("file:///project/unrelated.macro"));
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
    for (const watcher of env.host.watchers) watcher[event].fire(candidate);
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false, `${filename} ${event}`);
    env.registration.dispose(); env.previews.dispose();
  }
});

test("real client cancellation releases the initial preview proof on close", async () => {
  const env = await realClientSetup(); env.engine.hold = true;
  const session = await env.show(); assert.equal(env.engine.infos.length, 1);
  env.close(session); assert.equal(env.engine.infos[0].token.isCancellationRequested, true);
  env.engine.hold = false;
  const current = await env.service.revalidate(uri(main), "current", env.service.currentInstance, uri(main));
  assert.equal(current.revision, "current"); assert.equal(env.engine.infos.length, 2);
  env.engine.infos[0].reply.resolve(env.engine.snapshot()); await env.dispose();
});

test("a delayed root file observation after the initial proof requires Refresh before a source jump", async () => {
  const env = await realClientSetup(), show = vscode.window.showTextDocument;
  let observed = false;
  vscode.window.showTextDocument = async (document, options) => {
    if (!observed && document.uri.scheme === "dygnosis-effective") {
      await flush();
      assert.equal(env.engine.infos.length, 1, "the initial preview proof has completed");
      observed = true;
      // Replay native root-create delivery after its proof, before the preview
      // becomes active. The bytes and engine revision have not changed.
      env.service.invalidate(undefined, uri(main));
    }
    return show(document, options);
  };
  try {
    const session = await env.show();
    assert.equal(observed, true);
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    const requests = env.engine.infos.length;
    await env.run("goToWrittenSource");
    assert.equal(env.host.editor.document, session.document, "stale mappings cannot reveal a source");
    assert.equal(env.engine.infos.length, requests, "an unavailable action does not silently refresh its mapping");
    await env.run("refreshEffectiveModel");
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
    assert.equal(session.result.revision, "current");
    await env.run("goToWrittenSource");
    assert.equal(env.host.editor.document, env.rootDocument);
    assert.deepEqual(env.host.shown.at(-1).options.selection, new Range(1, 0, 1, 3));
  } finally { vscode.window.showTextDocument = show; await env.dispose(); }
});

test("unrelated root notifications during source loading obtain a bounded new proof for the unchanged preview", async context => {
  for (const interruptions of [1, 3, 4]) await context.test(`${interruptions} interruptions`, async () => {
    const env = await realClientSetup(), include = "file:///project/unopened.inc";
    let interrupted = 0;
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      const session = await env.show();
      env.host.load = async value => {
        const loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
        return loaded;
      };
      env.engine.infoHook = call => {
        if (!env.host.documents.has(include) || interrupted >= interruptions) return false;
        ++interrupted;
        void Promise.resolve().then(() => { env.service.invalidate("file:///project/other.mod"); call.reply.resolve(env.engine.snapshot()); });
        return true;
      };
      await env.run("goToWrittenSource");
      assert.equal(interrupted, interruptions);
      assert.equal(session.result.revision, "current");
      if (interruptions < 4) {
        assert.equal(env.host.editor.document.uri.toString(), include);
        assert.deepEqual(env.host.shown.at(-1).options.selection, new Range(0, 0, 0, 3));
      } else {
        assert.equal(env.host.editor.document, session.document);
        assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
      }
    } finally { await env.dispose(); }
  });
});

test("loader proof retries reject changed bytes and relevant hard input after an unrelated interruption", async context => {
  for (const change of ["revision", "source-version", "root-notification", "global-notification"]) await context.test(change, async () => {
    const env = await realClientSetup(), include = "file:///project/unopened.inc";
    let interrupted = false;
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      const session = await env.show(); let loaded;
      env.host.load = async value => {
        loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
        return loaded;
      };
      env.engine.infoHook = call => {
        if (!loaded || interrupted) return false;
        interrupted = true;
        void Promise.resolve().then(() => {
          env.service.invalidate("file:///project/other.mod");
          if (change === "revision") { env.engine.value.revision = "changed"; env.engine.value.effective_text = "model;\ny = 4;\nend;"; }
          if (change === "source-version") { ++loaded.version; loaded.text = "y=4;\n"; env.host.edited.fire({ document: loaded, contentChanges: [{ text: loaded.text }] }); }
          if (change === "root-notification") env.service.invalidate(main);
          if (change === "global-notification") env.service.invalidate();
          call.reply.resolve(env.engine.snapshot());
        });
        return true;
      };
      await env.run("goToWrittenSource");
      assert.equal(interrupted, true);
      assert.equal(env.host.editor.document, session.document);
      assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    } finally { await env.dispose(); }
  });
});

test("real client Refresh supersedes an unresolved initial proof without waiting for it", async () => {
  const env = await realClientSetup(); env.engine.hold = true;
  const session = await env.show(); env.engine.hold = false;
  env.engine.value = payload({ revision: "newer", effective_text: "model;\ny = 2;\nend;", navigation: [row({ id: "newer-row" })] });
  const refreshing = env.run("refreshEffectiveModel"); await flush();
  assert.equal(env.engine.infos[0].token.isCancellationRequested, true); assert.equal(env.engine.infos.length, 2);
  await refreshing; assert.equal(session.result.navigation[0].id, "newer-row");
  assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  env.engine.infos[0].reply.resolve(env.engine.snapshot()); await env.dispose();
});

test("real client supersede and close cancel only obsolete proofs while two other preview refreshes remain current", async () => {
  const env = await realClientSetup(), first = await env.show(), second = await env.show(), third = await env.show();
  const baseline = env.engine.infos.length; env.engine.hold = true;
  env.activate(first); const obsoleteNavigation = env.run("goToWrittenSource"); await flush();
  assert.equal(env.engine.infos.length, baseline + 1);
  env.activate(second); const secondRefresh = env.run("refreshEffectiveModel"); await flush();
  env.activate(third); const thirdRefresh = env.run("refreshEffectiveModel"); await flush();
  assert.equal(env.engine.infos.length, baseline + 1, "both other views must be queued behind the active proof");
  env.activate(first); const obsoleteRefresh = env.run("refreshEffectiveModel"); await flush();
  assert.equal(env.engine.infos[baseline].token.isCancellationRequested, true);
  assert.equal(env.engine.infos.length, baseline + 2, "the first live queued proof starts without the obsolete reply");
  env.close(first); await obsoleteNavigation; await obsoleteRefresh; await flush();
  assert.equal(env.engine.infos.length, baseline + 2); assert.equal(env.engine.infos[baseline + 1].token.isCancellationRequested, false);
  env.engine.infos[baseline + 1].reply.resolve(env.engine.snapshot()); await secondRefresh; await flush();
  assert.equal(env.engine.infos.length, baseline + 3); assert.equal(env.engine.infos[baseline + 2].token.isCancellationRequested, false);
  env.engine.infos[baseline + 2].reply.resolve(env.engine.snapshot()); await thirdRefresh;
  env.activate(second); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  env.activate(third); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
  assert.equal(env.engine.infos.length, baseline + 3, "the closed view's queued proof never starts");
  env.engine.infos[baseline].reply.resolve(env.engine.snapshot()); await env.dispose();
});

test("real client opens an unchanged unopened include despite load-time input observations", async context => {
  for (const observation of ["metadata", "file"]) await context.test(observation, async () => {
    const env = await realClientSetup(), include = "file:///project/unopened.inc";
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      await env.show();
      env.host.load = async value => {
        const loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded);
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
        env.host.opened.fire(loaded);
        if (observation === "metadata") env.host.edited.fire({ document: loaded, contentChanges: [] });
        else for (const watcher of [...env.host.watchers]) watcher.create.fire(loaded.uri);
        return loaded;
      };
      await env.run("goToWrittenSource");
      assert.equal(env.host.editor.document.uri.toString(), include, observation);
      assert.equal(env.host.shown.at(-1).options.viewColumn, vscode.ViewColumn.Beside);
    } finally { await env.dispose(); }
  });
});

test("changed disk or unsaved include bytes cannot use the old source mapping", async () => {
  for (const change of ["disk", "unsaved"]) {
    const env = await realClientSetup(), include = "file:///project/unopened.inc";
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      const session = await env.show(); let loaded;
      env.host.load = async value => {
        loaded = doc(value.toString(), "y=4;\n", change === "unsaved" ? 2 : 1); loaded.isDirty = change === "unsaved";
        env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
        env.engine.value.revision = "changed"; env.engine.value.effective_text = "model;\ny = 4;\nend;";
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
        for (const watcher of [...env.host.watchers]) watcher.change.fire(loaded.uri);
        if (change === "unsaved") env.host.edited.fire({ document: loaded, contentChanges: [{ text: loaded.text }] });
        return loaded;
      };
      await env.run("goToWrittenSource");
      assert.equal(env.host.editor.document, session.document, change);
      assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
      env.host.load = async () => loaded;
      await env.run("refreshEffectiveModel"); await env.run("goToWrittenSource");
      assert.equal(env.host.editor.document.uri.toString(), include, "Refresh may navigate the newly observed source version");
    } finally { await env.dispose(); }
  }
});

test("interleaved filesystem observations retry fresh proofs within a fixed bound", async () => {
  for (const observations of [3, 4]) {
    const env = await realClientSetup(), include = "file:///project/unopened.inc";
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      const session = await env.show(), baseline = env.engine.infos.length; let remaining = observations;
      env.host.load = async value => {
        const loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
        return loaded;
      };
      env.engine.infoHook = () => {
        if (!env.host.documents.has(include) || remaining === 0) return false;
        --remaining;
        for (const watcher of [...env.host.watchers]) watcher.change.fire(uri(include));
        assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false, "actions stay unavailable until a stable proof");
        return true;
      };
      await env.run("goToWrittenSource");
      assert.equal(env.engine.infos.length, baseline + 6, "two pre-load proofs and no more than four post-load attempts");
      assert.equal(env.host.editor.document === session.document, observations === 4);
      if (observations === 3) assert.equal(env.host.editor.document.uri.toString(), include);
      else assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    } finally { await env.dispose(); }
  }
});

test("new-epoch independent callers remain current while the source loader retries an obsolete filesystem proof", async () => {
  const env = await realClientSetup(), include = "file:///project/unopened.inc";
  try {
    env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
    await env.show(); let held;
    env.host.load = async value => {
      const loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
      env.engine.value.navigation[0].written_locations[0].document_version = loaded.version; return loaded;
    };
    env.engine.infoHook = call => { if (!held && env.host.documents.has(include)) { held = call; return true; } return false; };
    const jump = env.run("goToWrittenSource"); await flush(); assert.ok(held);
    for (const watcher of [...env.host.watchers]) watcher.change.fire(uri(include));
    const first = env.service.revalidate(uri(main), "current", env.service.currentInstance, uri(main));
    const second = env.service.revalidate(uri(main), "current", env.service.currentInstance, uri(main));
    assert.equal((await first).revision, "current"); assert.equal((await second).revision, "current"); await jump;
    assert.equal(held.token.isCancellationRequested, true); assert.equal(env.host.editor.document.uri.toString(), include);
    held.reply.resolve(env.engine.snapshot());
  } finally { await env.dispose(); }
});

test("Refresh still supersedes a source load made unavailable by filesystem observations", async () => {
  const env = await realClientSetup(), include = "file:///project/unopened.inc", held = deferred();
  try {
    env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
    const session = await env.show(); let loaded;
    env.host.load = value => {
      loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
      env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
      for (const watcher of [...env.host.watchers]) watcher.change.fire(loaded.uri);
      return held.promise;
    };
    const oldJump = env.run("goToWrittenSource"); await flush();
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    await env.run("refreshEffectiveModel"); held.resolve(loaded); await oldJump;
    assert.equal(env.host.editor.document, session.document); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), true);
    assert.equal(session.result.navigation[0].written_locations[0].document_version, 1);
  } finally { await env.dispose(); }
});

test("hard input changes still cancel a source load awaiting filesystem revalidation", async () => {
  for (const input of ["text", "settings", "restart", "close"]) {
    const env = await realClientSetup(), include = "file:///project/unopened.inc", held = deferred();
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      const session = await env.show(); let loaded;
      env.host.load = value => {
        loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
        for (const watcher of [...env.host.watchers]) watcher.change.fire(loaded.uri);
        return held.promise;
      };
      const jump = env.run("goToWrittenSource"); await flush();
      assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
      if (input === "text") { ++loaded.version; env.host.edited.fire({ document: loaded, contentChanges: [{ text: "y=3;\n" }] }); }
      if (input === "settings") env.host.configured.fire({ affectsConfiguration: () => true });
      if (input === "restart") await env.service.restart();
      if (input === "close") env.close(session);
      held.resolve(loaded); await jump;
      assert.notEqual(env.host.editor.document.uri.toString(), include, input);
      assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
    } finally { await env.dispose(); }
  }
});

test("a filesystem change after proof completion cannot reveal the source from the old input revision", async () => {
  const env = await realClientSetup(), include = "file:///project/unopened.inc", dependency = "file:///project/other.macro";
  try {
    env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include, dependency] });
    const session = await env.show(); let injected = false;
    env.host.load = async value => {
      const loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
      env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
      return loaded;
    };
    const original = env.service.revalidate.bind(env.service);
    env.service.revalidate = async (...args) => {
      const info = await original(...args);
      if (env.host.documents.has(include) && !injected) {
        // Let validated() finish its synchronous proof checks, then deliver the
        // file event before the awaiting native loader guard resumes.
        void Promise.resolve().then(() => Promise.resolve().then(() => {
          injected = true; env.engine.value.revision = "late-changed";
          env.engine.value.effective_text = "model;\ny = 4;\nend;";
          for (const watcher of [...env.host.watchers]) watcher.change.fire(uri(dependency));
        }));
      }
      return info;
    };
    await env.run("goToWrittenSource");
    assert.equal(injected, true);
    assert.equal(env.host.editor.document, session.document, "the just-completed old proof must not reveal a source");
    env.activate(session);
    assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false, "the late observation must keep the preview stale");
  } finally { await env.dispose(); }
});

test("a filesystem change after the loader guard completes is rechecked before source reveal", async () => {
  const env = await realClientSetup(), include = "file:///project/unopened.inc", dependency = "file:///project/other.macro";
  try {
    env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include, dependency] });
    const session = await env.show(); let injected = false;
    env.host.load = async value => {
      const loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
      env.engine.value.navigation[0].written_locations[0].document_version = loaded.version;
      return loaded;
    };
    const original = env.service.revalidate.bind(env.service);
    env.service.revalidate = async (...args) => {
      const info = await original(...args);
      if (env.host.documents.has(include) && !injected) {
        void Promise.resolve().then(() => Promise.resolve().then(() => Promise.resolve().then(() => {
          injected = true; env.engine.value.revision = "late-changed";
          env.engine.value.effective_text = "model;\ny = 4;\nend;";
          for (const watcher of [...env.host.watchers]) watcher.change.fire(uri(dependency));
        })));
      }
      return info;
    };
    await env.run("goToWrittenSource"); assert.equal(injected, true);
    assert.equal(env.host.editor.document, session.document);
    env.activate(session); assert.equal(env.host.contexts.get("dygnosis.previewWrittenSource"), false);
  } finally { await env.dispose(); }
});

test("the final guard decision rejects late loaded-version, closed and dirty changes", async () => {
  for (const change of ["version", "closed", "dirty"]) {
    const env = await realClientSetup(), include = "file:///project/unopened.inc";
    try {
      env.engine.value = payload({ effective_text: "model;\ny = 3;\nend;", navigation: [row({ written_locations: [target(include, new Range(0, 0, 0, 3), null)] })], dependency_candidates: [main, include] });
      const session = await env.show(); let loaded, injected = false;
      env.host.load = async value => {
        loaded = doc(value.toString(), "y=3;\n"); env.host.documents.set(value.toString(), loaded); env.host.opened.fire(loaded);
        env.engine.value.navigation[0].written_locations[0].document_version = loaded.version; return loaded;
      };
      const original = env.service.revalidate.bind(env.service);
      env.service.revalidate = async (...args) => {
        const info = await original(...args);
        if (loaded && !injected) {
          void Promise.resolve().then(() => Promise.resolve().then(() => Promise.resolve().then(() => {
            injected = true;
            if (change === "version") ++loaded.version;
            if (change === "closed") loaded.isClosed = true;
            if (change === "dirty") loaded.isDirty = true;
          })));
        }
        return info;
      };
      await env.run("goToWrittenSource"); assert.equal(injected, true);
      assert.equal(env.host.editor.document, session.document, change);
    } finally { await env.dispose(); }
  }
});
