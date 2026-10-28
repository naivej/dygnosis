const { compiledModule, importedModule } = require("./helpers/extension_modules.cjs");
const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");

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
class Range {
  constructor(line, character, lastLine, lastCharacter) {
    this.start = { line, character }; this.end = { line: lastLine, character: lastCharacter };
  }
}
class Selection extends Range { constructor(...args) { super(...args); this.active = this.start; } }
class CodeLens { constructor(range, command) { this.range = range; this.command = command; } }
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname), toString: () => value };
}
function document(value = "file:///workspace/main.mod", extra = {}) {
  return { uri: uri(value), languageId: "dynare", version: 1, isClosed: false,
    lineCount: 100, lineAt: () => ({ text: " ".repeat(60) }), ...extra };
}
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
const vscode = {
  Disposable, EventEmitter: Emitter, Range, Selection, CodeLens, TreeItem: class {},
  ViewColumn: { Beside: 2 },
  Uri: { parse: uri, file: value => uri(`file://${value.startsWith("/") ? "" : "/"}${value}`),
    from: value => uri(`${value.scheme}:${value.path}`) },
  languages: {
    registerDocumentSymbolProvider: () => new Disposable(),
    registerCodeLensProvider: (selector, provider) => {
      const registration = { selector, provider, disposed: false }; host.providers.push(registration);
      return new Disposable(() => { registration.disposed = true; });
    },
    setTextDocumentLanguage: async (doc, language) => { host.languages.push(language); doc.languageId = language; return doc; },
  },
  commands: {
    registerCommand: (id, callback) => { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); },
    executeCommand: async (id, ...args) => {
      host.executed.push({ id, args });
      if (id === "references-view.findReferences") {
        host.references.push({ uri: host.activeEditor.document.uri.toString(), position: host.activeEditor.selection.active }); return;
      }
      return host.commands.get(id)?.(...args);
    },
  },
  window: {
    get visibleTextEditors() { return host.editors; },
    get activeTextEditor() { return host.activeEditor; },
    onDidChangeVisibleTextEditors: listener => host.visible.event(listener),
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
    showQuickPick: async (items, options) => {
      host.picks.push({ items, options }); const answer = host.answers.shift();
      return typeof answer === "function" ? answer(items) : answer;
    },
    showInformationMessage: async message => { host.messages.push(message); },
    showTextDocument: async (doc, options) => { host.shown.push({ doc, options }); host.activeEditor = { document: doc }; return host.activeEditor; },
  },
  workspace: {
    getConfiguration: (section, scope) => {
      host.scopes.push({ section, scope });
      return { get: (key, fallback) => {
        const settings = host.resources.get(scope?.uri?.toString()) ?? {};
        const setting = `${section}.${key}`;
        return Object.hasOwn(settings, setting) ? settings[setting] : Object.hasOwn(host.settings, setting) ? host.settings[setting] : fallback;
      } };
    },
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidOpenTextDocument: listener => host.opened.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
    asRelativePath: value => value.path.split("/").at(-1),
    registerTextDocumentContentProvider: (scheme, provider) => {
      host.content.set(scheme, provider); return new Disposable(() => host.content.delete(scheme));
    },
    openTextDocument: async value => {
      host.loads.push(value.toString());
      return host.load ? host.load(value) : document(value.toString());
    },
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (importedModule(id, args[0]) === "client" && /(?:lenses|model_navigation|preview)\.js$/.test(args[0]?.filename ?? "")) return {
    documentSelector: [{ language: "dynare", scheme: "file" }, { language: "dynare", scheme: "untitled" }],
    isAnalysisDocument: doc => doc.languageId === "dynare" && ["file", "untitled"].includes(doc.uri.scheme),
  };
  return originalLoad.call(this, id, ...args);
};
const { lensPreferences, declarationLensGroups, registerLenses } = require(compiledModule("lenses"));
const { effectivePreviewArguments, registerEffectivePreview } = require(compiledModule("preview"));
Module._load = originalLoad;
const lensContributions = { configuration: require("../package.json").contributes.configuration.find(group => group.title === "Dygnosis: Actions") };

function location(value = "file:///workspace/main.mod", line = 3, start = 0, end = 5) {
  return { uri: value, range: { start: { line, character: start }, end: { line, character: end } } };
}
function statement(id = "s1", extra = {}) {
  const anchor = location();
  return { id, kind: "block", name: "model", complete: true, native: false, category: "model.aggregate", subtype: null,
    dimension: null, lens_anchor: anchor, equation_count: 1, location: anchor, anchor, segments: [anchor], origin_frames: [], ...extra };
}
function equation(id = "e1", extra = {}) {
  const written = location(undefined, 5);
  return { id, statement_id: "s1", block_id: "s1", scope: "aggregate", number: 1, dimension: null, name: "Euler", text: "y = y(-1)",
    location: written, anchor: written, segments: [written], origin_frames: [], ...extra };
}
function declaration(id = "d1", extra = {}) {
  const written = location(undefined, 1, 4, 5);
  return { id, statement_id: "decl1", name: "y", written_kind: "endo", final_kind: "endo", long_name: null, tex_name: null,
    written_dimension: null, dimension: null, timing: null, location: written, anchor: written, segments: [written], origin_frames: [], ...extra };
}
function facts(doc = document(), extra = {}) {
  return { schema_version: 1, root_uri: doc.uri.toString(), document_uri: doc.uri.toString(), document_version: doc.version,
    client_instance: 1, revision: "current", complete: true, owner_roots: [doc.uri.toString()], statements: [statement()], declarations: [],
    equations: [equation()], related_files: [], block_categories: [], first_model_anchor: location(), ...extra };
}
function setup(doc = document(), extraDocs = []) {
  const editor = { document: doc };
  host = { editors: [editor, ...extraDocs.map(document => ({ document }))], activeEditor: editor, settings: {}, resources: new Map(),
    scopes: [], providers: [], commands: new Map(), executed: [], references: [], picks: [], answers: [], messages: [],
    visible: new Emitter(), active: new Emitter(), edited: new Emitter(), opened: new Emitter(), closed: new Emitter(), configured: new Emitter(),
    loads: [], shown: [], languages: [], content: new Map() };
  const changed = new Emitter();
  const service = { currentInstance: 1, roots: [], requests: [], validations: [], logged: [], engineRequests: [],
    client: { initializeResult: { capabilities: { experimental: { dygnosis: { effectivePreview: { command: "dynare/showEffectiveModel", source_layout: true } } } } } },
    log: message => service.logged.push(message), onDidChange: changed.event,
    ensureStarted: async () => {},
    rootForDocument: async current => {
      service.roots.push(current.uri.toString());
      return service.root ?? (/\.(mod|dyn)$/i.test(current.uri.path) ? current.uri : undefined);
    },
    modelInfo: async (root, resource) => {
      service.requests.push({ root: root.toString(), document: resource.toString() });
      const current = host.editors.find(editor => editor.document.uri.toString() === resource.toString())?.document ?? doc;
      return service.info?.(root, resource, current) ?? facts(current, { root_uri: root.toString() });
    },
    revalidate: async (root, revision, instance, resource) => {
      service.validations.push({ root: root.toString(), revision, instance, resource: resource.toString() });
      const info = service.fresh ? await service.fresh(root, resource) : await service.modelInfo(root, resource);
      return info?.revision === revision && instance === service.currentInstance ? info : undefined;
    },
    execute: async (command, args) => {
      service.engineRequests.push({ command, args });
      return service.executeResult ? service.executeResult(command, args) : { effective_text: "model; y = 0; end;" };
    },
    failure: async message => service.logged.push(message),
  };
  const registration = registerLenses(service);
  const provider = host.providers[0].provider;
  const token = () => ({ isCancellationRequested: false });
  const provide = (current = doc, cancellation = token()) => provider.provideCodeLenses(current, cancellation);
  const run = lens => host.commands.get(lens.command.command)(...lens.command.arguments);
  const configure = (setting, value, resource) => {
    if (resource) { const settings = host.resources.get(resource) ?? {}; settings[setting] = value; host.resources.set(resource, settings); }
    else host.settings[setting] = value;
    host.configured.fire({ affectsConfiguration: section => setting.startsWith(section) });
  };
  return { host, doc, service, changed, registration, provider, token, provide, run, configure };
}

test("resource controls use approved defaults and native visibility with safe invalid-value fallback", () => {
  const env = setup();
  const properties = lensContributions.configuration.properties;
  for (const [key, fallback] of [["declarationReferences", false], ["effectiveModel", false]]) {
    assert.equal(properties[`dynare.codeLens.${key}`].type, "boolean"); assert.equal(properties[`dynare.codeLens.${key}`].scope, "resource");
    assert.equal(properties[`dynare.codeLens.${key}`].default, fallback);
    env.host.settings[`dynare.codeLens.${key}`] = "false";
  }
  assert.deepEqual(lensPreferences(env.doc, env.service.log), { declarationReferences: false, effectiveModel: false });
  assert.equal(env.service.logged.length, 2); assert.ok(env.host.scopes.every(row => row.scope.languageId === "dynare"));
  env.configure("editor.codeLens", false); assert.deepEqual(lensPreferences(env.doc, env.service.log), { declarationReferences: false, effectiveModel: false });
  env.registration.dispose();
});
test("default settings expose no equation lens or model-information request", async () => {
  const env = setup(); const lenses = await env.provide();
  assert.deepEqual(lenses, []); assert.equal(env.service.requests.length, 0);
  assert.equal(env.host.commands.has("dygnosis.browseLensEquations"), false);
  assert.equal(Object.hasOwn(lensContributions.configuration.properties, "dynare.codeLens.modelEquations"), false);
  assert.equal(env.host.references.length, 0); assert.equal(env.service.engineRequests.length, 0);
  env.registration.dispose();
});

test("incomplete, partial and unsafe model anchors expose no expansion lens", async () => {
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  for (const extra of [{ complete: false }, { statements: [statement("s1", { complete: false })] },
    { statements: [statement("s1", { native: true })] }, { statements: [statement("s1", { lens_anchor: null })] },
    { statements: [statement("s1", { lens_anchor: location(undefined, 100) })] },
    { statements: [statement("s1", { lens_anchor: location(undefined, 3, 59, 61) })] },
    { statements: [statement("s1", { lens_anchor: { ...location(), range: { start: { line: 3, character: 0 }, end: { line: 4, character: 5 } } } })] }]) {
    env.service.info = () => facts(env.doc, extra);
    assert.deepEqual(await env.provide(), []);
  }
  env.registration.dispose();
});
test("file-local anchors never use include segments or guessed first-model locations", async () => {
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  env.service.info = () => facts(env.doc, { statements: [statement("foreign", { lens_anchor: location("file:///workspace/part.inc"),
    segments: [location()] })], first_model_anchor: location("file:///workspace/part.inc") });
  assert.equal((await env.provide()).length, 0); env.registration.dispose();
});
test("includes require a selected known owner and keep its root even for included mod files", async () => {
  for (const value of ["file:///workspace/shared.inc", "file:///workspace/part.mod"]) {
    const env = setup(document(value)); env.configure("dynare.codeLens.effectiveModel", true);
    if (value.endsWith(".inc")) { assert.equal((await env.provide()).length, 0); assert.equal(env.service.requests.length, 0); }
    env.service.root = uri("file:///owners/b.mod"); env.service.info = () => facts(env.doc, { root_uri: env.service.root.toString(),
      statements: [statement("s1", { lens_anchor: location(value) })], first_model_anchor: location(value),
      equations: [equation("e1", { location: location(value, 5) })] });
    const lenses = await env.provide(); assert.equal(lenses.length, 1); assert.equal(env.service.requests.at(-1).root, "file:///owners/b.mod");
    assert.equal(lenses[0].command.arguments[0].context.root.toString(), "file:///owners/b.mod");
    env.registration.dispose();
  }
});
test("optional references group names and same-line statements, preserve retyping and request lazily", async () => {
  const env = setup(); env.configure("dynare.codeLens.declarationReferences", true);
  const declarations = [declaration(), declaration("d2", { name: "beta", final_kind: "parameter", location: location(undefined, 1, 7, 11) }),
    declaration("d3", { statement_id: "decl2", name: "z", location: location(undefined, 1, 15, 16) }),
    declaration("d4", { statement_id: "macrocopy", name: "y" })];
  env.service.info = () => facts(env.doc, { statements: [statement(), ...["decl1", "decl2", "macrocopy"].map(id =>
    statement(id, { kind: "declaration", name: "var", equation_count: null, lens_anchor: location(undefined, 1, 0, 3) }))], declarations });
  const lenses = await env.provide(); const lens = lenses.find(row => row.command.title === "Find references");
  assert.equal(lenses.length, 1); assert.equal(env.host.references.length, 0);
  assert.equal(declarationLensGroups(await env.service.modelInfo(env.doc.uri, env.doc.uri), env.doc)[0].declarations.length, 3);
  env.host.answers.push(items => items[1]); await env.run(lens);
  assert.deepEqual(env.host.picks[0].items.map(row => row.label), ["y", "beta", "z"]); assert.equal(env.host.picks[0].items[1].description, "parameter");
  assert.deepEqual(env.host.references, [{ uri: env.doc.uri.toString(), position: { line: 1, character: 7 } }]);
  assert.equal(env.service.engineRequests.length, 0); env.registration.dispose();
});
test("symbol-picker cancellation and stale selections never ask for references", async () => {
  for (const change of ["cancel", "version", "owner", "instance", "settings", "focus", "revision", "mapping"]) {
    const env = setup(); env.configure("dynare.codeLens.declarationReferences", true);
    const declarations = [declaration(), declaration("d2", { name: "z", location: location(undefined, 1, 7, 8) })];
    const snapshot = () => facts(env.doc, { statements: [statement("decl1", { kind: "declaration", lens_anchor: location(undefined, 1), equation_count: null })], declarations });
    env.service.info = snapshot; const lens = (await env.provide())[0];
    const pick = deferred(); env.host.answers.push(() => pick.promise); const pending = env.run(lens); await flush();
    if (change === "version") ++env.doc.version;
    if (change === "owner") env.service.root = uri("file:///other.mod");
    if (change === "instance") ++env.service.currentInstance;
    if (change === "settings") env.configure("dynare.codeLens.declarationReferences", false);
    if (change === "focus") env.host.activeEditor = { document: document("file:///other.mod") };
    if (change === "revision") env.service.fresh = async () => undefined;
    if (change === "mapping") env.service.fresh = async () => facts(env.doc, { ...snapshot(), declarations: [declaration("d1", { location: location(undefined, 2) }), declarations[1]] });
    pick.resolve(change === "cancel" ? undefined : env.host.picks[0].items[0]); await pending;
    assert.equal(env.host.references.length, 0, change); env.registration.dispose();
  }
});
test("native/feature off stops requests and invalidates old actions; re-enabling restores lenses live", async () => {
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  const preview = registerEffectivePreview(env.service); const old = (await env.provide())[0]; let events = 0;
  const listener = env.provider.onDidChangeCodeLenses(() => ++events);
  for (const setting of ["dynare.codeLens.effectiveModel", "editor.codeLens"]) {
    env.configure(setting, false); const count = env.service.requests.length;
    assert.deepEqual(await env.provide(), []); assert.equal(env.service.requests.length, count); await env.run(old); assert.equal(env.service.engineRequests.length, 0);
    env.configure(setting, true); assert.equal((await env.provide()).length, 1);
  }
  assert.equal(events, 4); assert.equal(env.service.currentInstance, 1); listener.dispose(); preview.dispose(); env.registration.dispose();
});
test("presentation controls resolve against the displayed resource in multiple folders", async () => {
  const other = document("file:///other/model.mod"); const env = setup(document(), [other]);
  env.configure("dynare.codeLens.effectiveModel", true); env.configure("dynare.codeLens.effectiveModel", false, other.uri.toString());
  assert.equal((await env.provide()).length, 1); assert.deepEqual(await env.provide(other), []);
  assert.equal(env.service.requests.length, 1); env.registration.dispose();
});
test("cancelled, superseded, hidden, closed, retyped and mismatched replies cannot restore lenses", async () => {
  for (const change of ["cancel", "superseded", "version", "owner", "instance", "hidden", "closed", "retyped", "rootReply", "documentReply", "versionReply", "instanceReply"]) {
    const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
    const late = deferred(); env.service.modelInfo = () => late.promise; const token = env.token();
    const pending = env.provide(env.doc, token); await flush(); const snapshot = facts(env.doc);
    if (change === "cancel") token.isCancellationRequested = true;
    if (change === "superseded") { env.service.modelInfo = async () => snapshot; await env.provide(); }
    if (change === "version") ++env.doc.version;
    if (change === "owner") env.service.root = uri("file:///next.mod");
    if (change === "instance") ++env.service.currentInstance;
    if (change === "hidden") env.host.editors = [];
    if (change === "closed") env.doc.isClosed = true;
    if (change === "retyped") env.doc.languageId = "plaintext";
    if (change === "rootReply") snapshot.root_uri = "file:///wrong.mod";
    if (change === "documentReply") snapshot.document_uri = "file:///wrong.inc";
    if (change === "versionReply") snapshot.document_version = 0;
    if (change === "instanceReply") snapshot.client_instance = 10;
    late.resolve(snapshot); assert.deepEqual(await pending, [], change); env.registration.dispose();
  }
});
test("unsupported resources and unavailable facts stay quiet without source parsing", async () => {
  for (const doc of [document("dygnosis-effective:/main.mod"), document("vscode-vfs:/main.mod"), document(undefined, { languageId: "plaintext" }), document(undefined, { isClosed: true })]) {
    const env = setup(doc); env.configure("dynare.codeLens.effectiveModel", true);
    assert.equal((await env.provide()).length, 0); assert.equal(env.service.requests.length, 0); env.registration.dispose();
  }
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  env.service.modelInfo = async () => undefined; assert.deepEqual(await env.provide(), []);
  env.service.modelInfo = async () => { throw new Error("Unsupported schema"); }; assert.deepEqual(await env.provide(), []);
  assert.ok(env.service.logged.includes("Error: Unsupported schema")); assert.equal(env.host.messages.length, 0); env.registration.dispose();
});
test("effective lens needs a safe opener but does not need an authoritative equation count", async () => {
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  env.service.info = () => facts(env.doc, { statements: [statement("s1", { equation_count: null })], equations: [equation("e1", { location: null })] });
  assert.deepEqual((await env.provide()).map(row => row.command.title), ["Expand macros"]); env.registration.dispose();
});
test("effective preview requires a current advertised layout", () => {
  const root = uri("file:///workspace/main.mod");
  const service = layout => ({ client: { initializeResult: { capabilities: { experimental: { dygnosis: { effectivePreview: layout } } } } } });
  assert.deepEqual(effectivePreviewArguments(service({ command: "dynare/showEffectiveModel", source_layout: true }), root), [{ root_uri: root.toString(), layout: "source" }]);
  assert.throws(() => effectivePreviewArguments(service({ command: "dynare/showEffectiveModel", readable_layout: true }), root), /does not advertise the source-preserving macro expansion layout/);
  assert.throws(() => effectivePreviewArguments(service({ navigation_schema_version: 1 }), root), /does not advertise the source-preserving macro expansion layout/);
});
test("optional effective lens uses only the first safe opener and passes the explicit validated context", async () => {
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  env.service.info = () => facts(env.doc, { statements: [statement(), statement("s2", { lens_anchor: location(undefined, 20), equation_count: 0 })] });
  const preview = registerEffectivePreview(env.service); const lenses = await env.provide();
  assert.equal(lenses.filter(row => row.command.title === "Expand macros").length, 1);
  await env.run(lenses.find(row => row.command.title === "Expand macros"));
  assert.deepEqual(env.service.engineRequests, [{ command: "dynare/showEffectiveModel", args: [{ root_uri: env.doc.uri.toString(), layout: "source" }] }]);
  assert.equal(env.host.shown[0].doc.uri.scheme, "dygnosis-effective"); assert.deepEqual(env.host.languages, ["dynare"]);
  assert.equal(env.host.shown[0].options.viewColumn, 2); preview.dispose(); env.registration.dispose();
});
test("effective preview expected context refuses owner/version/revision/instance and disposal races", async () => {
  for (const change of ["owner", "version", "revision", "instance", "setting", "dispose", "loading", "focus"]) {
    const env = setup(); env.configure("dynare.codeLens.effectiveModel", true); const preview = registerEffectivePreview(env.service);
    const lens = (await env.provide()).find(row => row.command.title === "Expand macros");
    const reply = deferred(); env.service.executeResult = () => reply.promise; const pending = env.run(lens); await flush();
    assert.equal(env.service.engineRequests.length, 1);
    if (change === "owner") env.service.root = uri("file:///other.mod");
    if (change === "version") ++env.doc.version;
    if (change === "revision") env.service.fresh = async () => undefined;
    if (change === "instance") ++env.service.currentInstance;
    if (change === "setting") env.configure("dynare.codeLens.effectiveModel", false);
    if (change === "dispose") preview.dispose();
    if (change === "focus") env.host.activeEditor = { document: document("file:///other.mod") };
    if (change === "loading") {
      const loaded = deferred(); env.host.load = () => loaded.promise; reply.resolve({ effective_text: "model;" }); await flush();
      env.service.root = uri("file:///other.mod"); loaded.resolve(document("dygnosis-effective:/model.mod"));
    } else reply.resolve({ effective_text: "model;" });
    await pending; assert.equal(env.host.shown.length, 0, change); preview.dispose(); env.registration.dispose();
  }
});
test("ordinary effective-preview menu URI arguments retain the native root picker and preview action", async () => {
  const env = setup(); const preview = registerEffectivePreview(env.service);
  await env.host.commands.get("dygnosis.showEffectiveModel")(env.doc.uri);
  assert.deepEqual(env.service.engineRequests, [{ command: "dynare/showEffectiveModel", args: [{ root_uri: env.doc.uri.toString(), layout: "source" }] }]);
  assert.equal(env.host.shown.length, 1); assert.equal(env.service.validations.length, 0);
  preview.dispose(); env.registration.dispose();
});
test("disposal removes provider/commands/listeners and rejects pending replies and forged targets", async () => {
  const env = setup(); env.configure("dynare.codeLens.effectiveModel", true);
  const lens = (await env.provide())[0]; await env.host.commands.get("dygnosis.showLensEffectiveModel")({ ...lens.command.arguments[0] });
  assert.equal(env.host.executed.length, 0); const late = deferred(); env.service.modelInfo = () => late.promise;
  const pending = env.provide(); await flush(); env.registration.dispose(); late.resolve(facts(env.doc));
  assert.deepEqual(await pending, []); assert.ok(env.host.providers[0].disposed);
  for (const event of [env.changed, env.host.visible, env.host.active, env.host.edited, env.host.opened, env.host.closed, env.host.configured]) assert.equal(event.listeners.size, 0);
  assert.equal(env.host.commands.has("dygnosis.showLensEffectiveModel"), false); assert.equal(env.host.commands.has("dygnosis.findLensReferences"), false);
  env.registration.dispose();
});
