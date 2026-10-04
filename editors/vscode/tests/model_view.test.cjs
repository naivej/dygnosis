const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");

class Disposable {
  constructor(callback) { this.callback = callback; }
  dispose() { this.callback?.(); this.callback = undefined; }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
  dispose() { this.listeners.clear(); }
}
class TreeItem {
  constructor(label, collapsibleState) { this.label = label; this.collapsibleState = collapsibleState; }
}
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, fsPath: decodeURIComponent(parsed.pathname), toString: () => value };
}
function document(value = "file:///project/main.mod", extra = {}) {
  return { uri: uri(value), languageId: "dynare", version: 1, isClosed: false, ...extra };
}
function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}
const flush = async () => { await new Promise(resolve => setImmediate(resolve)); await new Promise(resolve => setImmediate(resolve)); };
let host;
const vscode = {
  Disposable, EventEmitter: Emitter, TreeItem,
  TreeItemCollapsibleState: { None: 0, Collapsed: 1 }, ThemeIcon: class { constructor(id) { this.id = id; } },
  Uri: { parse: uri, file: path => uri(`file://${path.startsWith("/") ? "" : "/"}${path.replaceAll("\\", "/")}`) },
  commands: {
    registerCommand: (id, callback) => { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); },
    executeCommand: async (id, ...args) => { if (id === "setContext") host.contexts.set(args[0], args[1]); else return host.commands.get(id)?.(...args); },
  },
  window: {
    get activeTextEditor() { return host.editor; },
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
    createTreeView: (id, options) => {
      const view = { id, ...options, visible: host.visible, disposed: false, onDidChangeVisibility: host.visibility.event,
        dispose() { this.disposed = true; } };
      host.views.push(view);
      return view;
    },
    showQuickPick: async (items, options) => {
      host.picks.push({ items, options });
      const answer = host.answers.shift();
      return typeof answer === "function" ? answer(items) : answer;
    },
    showInputBox: async options => { host.inputs.push(options); return host.inputAnswers.shift(); },
    showInformationMessage: async message => { host.messages.push(message); },
    showTextDocument: async doc => {
      host.opened.push(doc.uri.toString()); host.editor = { document: doc }; host.active.fire(host.editor);
    },
  },
  workspace: {
    asRelativePath: value => value.path.replace(/^\/project\//, ""),
    getConfiguration: (_section, scope) => {
      host.settingScopes.push(scope);
      const settings = host.resourceSettings.get(scope.uri?.toString()) ?? {};
      return { get: (key, fallback) => settings[key] ?? host.settings[key] ?? fallback };
    },
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
    openTextDocument: async target => {
      host.fileRequests.push(target.toString());
      return host.openDocument ? host.openDocument(target) : document(target.toString());
    },
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (id === "./client" && args[0]?.filename.endsWith("model_view.js")) return {
    isAnalysisDocument: doc => doc.languageId === "dynare" && ["file", "untitled"].includes(doc.uri.scheme),
  };
  return originalLoad.call(this, id, ...args);
};
const { registerModelView, sameWrittenLocation } = require("../out/model_view");
Module._load = originalLoad;

function location(value = "file:///project/main.mod", line = 5) {
  return { uri: value, range: { start: { line, character: 2 }, end: { line, character: 10 } } };
}
function equation(id = "aggregate:1", overrides = {}) {
  return { id, number: 1, name: "Euler", text: "c = y;", scope: "aggregate", dimension: null,
    statement_id: "row:1", block_id: "block:1", location: location(), anchor: location(), segments: [location()], origin_frames: [], ...overrides };
}
function facts(doc = document(), overrides = {}) {
  return { schema_version: 1, root_uri: doc.uri.toString(), document_uri: doc.uri.toString(), document_version: doc.version,
    revision: "current", client_instance: 1, complete: true, owner_roots: [doc.uri.toString()], statements: [], declarations: [],
    equations: [equation()], related_files: [], block_categories: [], first_model_anchor: null,
    n_endogenous: 4, n_exogenous: 2, n_parameters: 1, n_equations: 3,
    endogenous: ["a", "b", "c", "d"], exogenous: ["e", "f"], parameters: ["p"],
    static: ["a"], predetermined: ["b"], forward_looking: ["c"], mixed: ["d"],
    heterogeneity_dimensions: [{ dimension: "households", n_endogenous: 100, n_exogenous: 10, n_parameters: 20, n_equations: 90,
      endogenous: [], exogenous: [], parameters: [], static: ["h"], predetermined: [], forward_looking: [], mixed: [] }], ...overrides };
}
function setup(doc = document(), visible = true) {
  host = { editor: doc ? { document: doc } : undefined, visible, settings: {}, resourceSettings: new Map(), settingScopes: [],
    commands: new Map(), contexts: new Map(), views: [], picks: [], answers: [], inputs: [], inputAnswers: [], messages: [], opened: [], fileRequests: [],
    active: new Emitter(), visibility: new Emitter(), edited: new Emitter(), closed: new Emitter(), configured: new Emitter() };
  const changed = new Emitter();
  const service = {
    currentInstance: 1, supportsModelInfo: true, requests: [], validations: [], jumps: [], owners: [], selected: new Map(), logged: [],
    log: message => service.logged.push(message), onDidChange: changed.event,
    knownOwners: () => service.owners,
    ownerChoices: async () => service.owners,
    rootForDocument: async current => service.selected.get(current.uri.toString()) ?? service.root ?? (/\.(mod|dyn)$/i.test(current.uri.path) ? current.uri : undefined),
    modelInfo: async (root, current) => {
      service.requests.push({ root: root.toString(), document: current.toString() });
      return service.info?.(root, current) ?? facts(host.editor.document, { root_uri: root.toString(), document_uri: current.toString() });
    },
    revalidate: async (root, revision, instance, current) => {
      service.validations.push({ root: root.toString(), revision, instance, document: current.toString() });
      return service.fresh ?? service.modelInfo(root, current);
    },
    openLocation: async (target, root, guard) => {
      const loaded = await vscode.workspace.openTextDocument(uri(target.uri));
      if (guard && !await guard(loaded)) return;
      service.jumps.push({ target, root: root.toString() });
    },
    selectOwner: (current, root) => { service.selected.set(current.toString(), root); changed.fire(); },
    invalidate: () => changed.fire(),
  };
  const configure = (value, resource) => {
    if (resource) host.resourceSettings.set(resource, { "modelView.sections": value });
    else host.settings["modelView.sections"] = value;
    host.configured.fire({ affectsConfiguration: () => true });
  };
  const registration = registerModelView(service);
  return { host, service, changed, configure, registration, doc, view: host.views[0],
    rows: () => host.views[0].treeDataProvider.getChildren(),
    run: (id, ...args) => host.commands.get(`dygnosis.${id}`)(...args) };
}
function descendants(item) { return [item, ...item.children.flatMap(descendants)]; }
function find(env, label) { return env.rows().flatMap(descendants).find(row => row.label === label); }

test("native Explorer tree labels aggregate counts, dimension counts and timing separately", async () => {
  const env = setup(); await flush();
  assert.equal(env.view.id, "dygnosis.model"); assert.equal(env.view.showCollapseAll, true);
  assert.deepEqual(env.rows().map(row => row.label), ["Aggregate counts", "Aggregate timing", "Heterogeneity dimensions", "Related files"]);
  const aggregate = find(env, "Aggregate counts");
  assert.equal(aggregate.children[0].description, "4"); assert.equal(aggregate.children[3].description, "3");
  assert.equal(find(env, "households").children[0].children[0].description, "100");
  assert.equal(find(env, "Forward-looking").children[0].label, "c");
  assert.equal(aggregate.iconPath.id, "graph"); assert.equal(aggregate.color, undefined);
  assert.equal(env.view.description, "main.mod"); assert.equal(env.view.message, undefined);
  env.registration.dispose();
});
test("resource sections reorder, clear immediately, reset, and normalize invalid entries", async () => {
  const env = setup(); await flush();
  env.configure(["relatedFiles", "timing"], env.doc.uri.toString()); await flush();
  assert.deepEqual(env.rows().map(row => row.label), ["Related files", "Aggregate timing"]);
  assert.ok(env.host.settingScopes.every(scope => scope.uri === env.doc.uri && scope.languageId === "dynare"));
  const requests = env.service.requests.length;
  env.configure([]); env.host.resourceSettings.clear(); env.configure([]);
  assert.equal(env.rows().length, 0); await flush(); assert.equal(env.service.requests.length, requests);
  env.configure(["counts", "counts", "bad", 3]); await flush();
  assert.deepEqual(env.rows().map(row => row.label), ["Aggregate counts"]);
  assert.ok(env.service.logged.some(value => value.includes("ignored")));
  env.configure("bad"); await flush(); assert.equal(env.rows().length, 4);
  delete env.host.settings["modelView.sections"]; env.configure(undefined); await flush(); assert.equal(env.rows().length, 4);
  env.registration.dispose();
});
test("native visibility stops the view's requests and clears content until shown again", async () => {
  const env = setup(undefined, false); await flush(); assert.equal(env.service.requests.length, 0);
  env.view.visible = true; env.host.visibility.fire({ visible: true }); await flush(); assert.equal(env.rows().length, 4);
  env.view.visible = false; env.host.visibility.fire({ visible: false }); assert.equal(env.rows().length, 0);
  const count = env.service.requests.length; env.changed.fire(); await flush(); assert.equal(env.service.requests.length, count);
  env.registration.dispose();
});
test("include focus uses its chosen owner and displayed-file presentation preferences", async () => {
  const env = setup(document("file:///other/part.inc")); env.service.root = uri("file:///project/owner.mod");
  env.configure(["counts"], env.doc.uri.toString()); await flush();
  assert.equal(env.service.requests.at(-1).root, "file:///project/owner.mod");
  assert.equal(env.service.requests.at(-1).document, "file:///other/part.inc");
  assert.equal(env.view.description, "owner.mod"); assert.equal(env.rows().length, 1);
  env.registration.dispose();
});
test("no root, non-Dynare documents and virtual previews clear the model", async () => {
  const env = setup(); await flush();
  for (const doc of [document("file:///project/part.inc"), document("dygnosis-effective:/main.mod"), document("file:///project/data.txt", { languageId: "plaintext" }), undefined]) {
    env.host.editor = doc ? { document: doc } : undefined; env.host.active.fire(env.host.editor); await flush();
    assert.equal(env.rows().length, 0); assert.match(env.view.message, /No model root|Open a Dynare/);
  }
  env.registration.dispose();
});
test("ambiguous include offers owner choice without selecting an arbitrary root", async () => {
  const env = setup(document("file:///project/part.inc"));
  env.service.owners = [uri("file:///project/a.mod"), uri("file:///project/b.mod")]; await flush();
  assert.equal(env.rows().length, 0); assert.match(env.view.message, /Choose the model/);
  env.service.info = (root, current) => facts(env.doc, { root_uri: root.toString(), document_uri: current.toString(), owner_roots: [root.toString()] });
  env.host.answers.push(items => items[1]); await env.run("chooseModelOwner"); await flush();
  assert.equal(env.service.selected.get(env.doc.uri.toString()).toString(), "file:///project/b.mod");
  assert.equal(env.view.description, "b.mod"); assert.equal(env.rows().length, 4);
  assert.equal(env.service.validations.at(-1).root, "file:///project/b.mod");
  env.registration.dispose();
});
test("owner cancellation and edit during owner selection do not change ownership", async () => {
  const env = setup(document("file:///project/part.inc")); const root = uri("file:///project/a.mod");
  env.service.owners = [root]; env.service.info = () => facts(env.doc, { root_uri: root.toString(), owner_roots: [root.toString()] });
  await env.run("chooseModelOwner"); assert.equal(env.service.selected.size, 0);
  const pick = deferred(); env.host.answers.push(() => pick.promise); const pending = env.run("chooseModelOwner"); await flush();
  ++env.doc.version; pick.resolve(env.host.picks.at(-1).items[0]); await pending;
  assert.equal(env.service.selected.size, 0); env.registration.dispose();
});
test("incomplete expansion does not expose old counts or equations", async () => {
  const env = setup(); await flush();
  env.service.info = () => facts(env.doc, { complete: false, message: "Missing required include." }); env.changed.fire(); await flush();
  assert.equal(env.rows().length, 0); assert.match(env.view.message, /Missing required include.*Counts and equation navigation are unavailable/);
  assert.equal(env.host.contexts.get("dygnosis.equationNavigation"), false);
  env.host.inputAnswers.push("1"); await env.run("goToEquation");
  assert.equal(env.host.inputs.length, 0); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("failed and older engines show recovery without stale facts", async () => {
  const env = setup(); await flush();
  env.service.modelInfo = async () => undefined; env.service.supportsModelInfo = false; env.changed.fire(); await flush();
  assert.equal(env.rows().length, 0); assert.match(env.view.message, /Restart.*bundled/);
  env.service.supportsModelInfo = true; env.service.modelInfo = async () => { throw new Error("unknown schema"); }; env.changed.fire(); await flush();
  assert.equal(env.rows().length, 0); assert.match(env.view.message, /Output/); assert.ok(env.service.logged.includes("Error: unknown schema"));
  env.registration.dispose();
});
test("superseded root, active document, version and client replies cannot restore facts", async () => {
  const env = setup(); await flush();
  const old = deferred(), next = deferred(); env.service.modelInfo = () => old.promise; env.changed.fire(); await flush();
  assert.equal(env.rows().length, 0);
  env.service.root = uri("file:///project/new.mod"); env.service.modelInfo = () => next.promise; env.changed.fire(); await flush();
  next.resolve(facts(env.doc, { root_uri: "file:///project/new.mod", n_endogenous: 7 })); await flush();
  old.resolve(facts(env.doc, { n_endogenous: 99 })); await flush(); assert.equal(find(env, "Endogenous").description, "7");
  for (const kind of ["document", "version", "client"]) {
    const late = deferred(); env.service.modelInfo = () => late.promise; env.changed.fire(); await flush();
    if (kind === "document") { env.host.editor = { document: document("file:///project/other.mod") }; env.host.active.fire(env.host.editor); }
    else if (kind === "version") { ++env.host.editor.document.version; env.host.edited.fire({ document: env.host.editor.document }); }
    else ++env.service.currentInstance;
    late.resolve(facts(env.doc)); await flush(); assert.equal(env.rows().length, 0);
  }
  env.registration.dispose();
});
test("equation input validates positive whole numbers before opening verified UTF-16 ranges", async () => {
  const env = setup(); await flush(); env.host.inputAnswers.push("1"); await env.run("goToEquation");
  const options = env.host.inputs[0];
  for (const value of ["", "0", "-1", "1.5", "1e2", " 1", "9007199254740992"]) assert.match(options.validateInput(value), /positive whole/);
  assert.equal(options.validateInput("2"), undefined); assert.match(options.prompt, /before transformation/);
  assert.deepEqual(env.service.jumps[0], { target: location(), root: env.doc.uri.toString() });
  assert.deepEqual(env.service.validations[0], { root: env.doc.uri.toString(), revision: "current", instance: 1, document: env.doc.uri.toString() });
  env.registration.dispose();
});
test("numbered navigation explicitly chooses aggregate or heterogeneous scope", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { equations: [equation(), equation("dimension:1", { scope: "dimension", dimension: "households", location: location("file:///project/hank.inc", 20) })] });
  await flush(); env.host.answers.push(items => items[1]); env.host.inputAnswers.push("1"); await env.run("goToEquation");
  assert.deepEqual(env.host.picks[0].items.map(item => item.label), ["Aggregate", "Dimension households"]);
  assert.match(env.host.inputs[0].prompt, /households/); assert.equal(env.service.jumps[0].target.uri, "file:///project/hank.inc");
  env.registration.dispose();
});
test("named search distinguishes scopes, files and macro occurrences at the same written site", async () => {
  const env = setup(); const repeated = [equation("copy:1", { origin_frames: [{ kind: "for", variable: "i", value: "1", segments: [location()] }] }),
    equation("copy:2", { number: 2, origin_frames: [{ kind: "for", variable: "i", value: "2", segments: [location()] }] }),
    equation("dimension:1", { dimension: "firms", scope: "dimension", location: location("file:///project/firms.inc") })];
  env.service.info = () => facts(env.doc, { equations: repeated }); await flush();
  env.host.answers.push(items => items[1]); await env.run("jumpToNamedEquation");
  const picker = env.host.picks[0]; assert.equal(picker.items.length, 3); assert.equal(picker.options.matchOnDescription, true); assert.equal(picker.options.matchOnDetail, true);
  assert.match(picker.items[0].detail, /i=1/); assert.match(picker.items[1].detail, /i=2/);
  assert.equal(picker.items[0].equation.id, "copy:1"); assert.equal(picker.items[1].equation.id, "copy:2");
  assert.match(picker.items[2].description, /Dimension firms.*firms.inc:6/);
  assert.equal(env.service.jumps.length, 1); env.registration.dispose();
});
test("scope, input, and named picker cancellations stop navigation", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { equations: [equation(), equation("d", { scope: "dimension", dimension: "h" })] }); await flush();
  await env.run("goToEquation"); assert.equal(env.host.inputs.length, 0);
  env.host.answers.push(items => items[0]); await env.run("goToEquation");
  await env.run("jumpToNamedEquation"); assert.equal(env.service.validations.length, 0); assert.equal(env.service.jumps.length, 0);
  env.registration.dispose();
});
test("missing number and missing named rows explain their empty state", async () => {
  const env = setup(); await flush(); env.host.inputAnswers.push("9"); await env.run("goToEquation");
  assert.match(env.host.messages.at(-1), /Equation 9.*Aggregate/);
  env.service.info = () => facts(env.doc, { equations: [equation("unnamed", { name: "" })] }); await env.run("jumpToNamedEquation");
  assert.match(env.host.messages.at(-1), /no named counted/); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("root/version/restart changes while a picker is open refuse the old equation", async () => {
  for (const kind of ["root", "version", "client", "document"]) {
    const env = setup(); await flush(); const pick = deferred(); env.host.answers.push(() => pick.promise);
    const pending = env.run("jumpToNamedEquation"); await flush(); const chosen = env.host.picks.at(-1).items[0];
    if (kind === "root") env.service.root = uri("file:///project/changed.mod");
    else if (kind === "version") ++env.doc.version;
    else if (kind === "client") ++env.service.currentInstance;
    else env.host.editor = { document: document("file:///project/other.mod") };
    pick.resolve(chosen); await pending; assert.equal(env.service.jumps.length, 0); env.registration.dispose();
  }
});
test("dependency/settings revision changes and missing current identities refuse navigation", async () => {
  const env = setup(); await flush();
  env.service.revalidate = async () => undefined; env.host.inputAnswers.push("1"); await env.run("goToEquation");
  assert.equal(env.service.jumps.length, 0); assert.match(env.host.messages.at(-1), /model changed/);
  env.service.revalidate = async () => facts(env.doc, { equations: [equation("replacement")] });
  env.host.inputAnswers.push("1"); await env.run("goToEquation"); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("owner changes during the fresh revision request refuse the old target", async () => {
  const env = setup(); await flush(); const fresh = deferred(); env.service.revalidate = () => fresh.promise;
  env.host.inputAnswers.push("1"); const pending = env.run("goToEquation"); await flush();
  env.service.root = uri("file:///project/changed.mod"); fresh.resolve(facts(env.doc)); await pending;
  assert.equal(env.service.jumps.length, 0); assert.match(env.host.messages.at(-1), /model changed/); env.registration.dispose();
});
test("source edits, focus, ownership, restart, and dependency revisions during source loading refuse the jump", async () => {
  for (const change of ["source", "focus", "owner", "restart", "dependency"]) {
    const env = setup(); await flush(); const loader = deferred(); env.host.openDocument = () => loader.promise;
    env.host.inputAnswers.push("1"); const pending = env.run("goToEquation"); await flush();
    assert.equal(env.host.fileRequests.length, 1);
    if (change === "source") ++env.doc.version;
    else if (change === "focus") env.host.editor = { document: document("file:///project/other.mod") };
    else if (change === "owner") env.service.selected.set(env.doc.uri.toString(), uri("file:///project/other.mod"));
    else if (change === "restart") ++env.service.currentInstance;
    else env.service.revalidate = async () => undefined;
    loader.resolve(document()); await pending;
    assert.equal(env.service.jumps.length, 0, change); assert.equal(env.host.opened.length, 0, change);
    env.registration.dispose();
  }
});
test("a valid unopened include still jumps after its ordinary overlay is opened", async () => {
  const env = setup(); const target = "file:///project/unopened.inc";
  env.service.info = () => facts(env.doc, { equations: [equation("include:1", { location: location(target, 11) })] });
  await flush(); const loader = deferred(); env.host.openDocument = () => loader.promise;
  env.host.inputAnswers.push("1"); const pending = env.run("goToEquation"); await flush();
  env.changed.fire(); // Loading may refresh presentation; it does not change input content.
  loader.resolve(document(target)); await pending;
  assert.deepEqual(env.service.jumps, [{ target: location(target, 11), root: env.doc.uri.toString() }]);
  assert.equal(env.service.validations.length, 2); env.registration.dispose();
});
test("file identity follows Windows case folding, with exact ranges and case-sensitive Unix and virtual URIs", () => {
  const written = location("file:///C:/users/shifu/project/part.inc", 11);
  const opened = location("file:///C:/Users/Shifu/project/part.inc", 11);
  assert.equal(sameWrittenLocation(written, opened, "win32"), true);
  assert.equal(sameWrittenLocation(written, opened, "linux"), false);
  assert.equal(sameWrittenLocation(written, opened, "darwin"), false);
  assert.equal(sameWrittenLocation(written, location("file:///C:/users/shifu/project/other.inc", 11), "win32"), false);
  assert.equal(sameWrittenLocation(written, location(written.uri, 12), "win32"), false);
  assert.equal(sameWrittenLocation(written, { ...written, range: { ...written.range, end: { line: 11, character: 11 } } }, "win32"), false);
  assert.equal(sameWrittenLocation(location("untitled:/Model.mod"), location("untitled:/model.mod"), "win32"), false);
  assert.equal(sameWrittenLocation(location("dygnosis-effective:/Model.mod"), location("dygnosis-effective:/model.mod"), "win32"), false);
});
test("post-loader mapping accepts Windows path spelling but refuses a different file or range", async () => {
  for (const change of ["case", "file", "range"]) {
    const env = setup(); const written = location("file:///C:/users/shifu/project/part.inc", 11);
    env.service.info = () => facts(env.doc, { equations: [equation("include:1", { location: written })] }); await flush();
    const loader = deferred(); env.host.openDocument = () => loader.promise;
    env.host.inputAnswers.push("1"); const pending = env.run("goToEquation"); await flush();
    const opened = location(change === "file" ? "file:///C:/Users/Shifu/project/other.inc" : "file:///C:/Users/Shifu/project/part.inc", change === "range" ? 12 : 11);
    env.service.fresh = facts(env.doc, { equations: [equation("include:1", { location: opened })] });
    loader.resolve(document(opened.uri)); await pending;
    assert.equal(env.service.jumps.length, change === "case" && process.platform === "win32" ? 1 : 0, change);
    assert.equal(env.service.validations.length, 2); env.registration.dispose();
  }
});
test("source target edits during the post-load revision request refuse display", async () => {
  const env = setup(); await flush(); const loaded = document(); env.host.openDocument = async () => loaded;
  const fresh = deferred(); let requests = 0;
  env.service.revalidate = async () => ++requests === 1 ? facts(env.doc) : fresh.promise;
  env.host.inputAnswers.push("1"); const pending = env.run("goToEquation"); await flush();
  ++loaded.version; fresh.resolve(facts(env.doc)); await pending;
  assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("equation commands remain usable when native view visibility or content is off", async () => {
  for (const state of ["hidden", "empty"]) {
    const env = setup(undefined, state !== "hidden"); if (state === "empty") env.configure([]); await flush();
    assert.equal(env.rows().length, 0); env.host.inputAnswers.push("1"); await env.run("goToEquation");
    assert.equal(env.service.jumps.length, 1); assert.equal(env.rows().length, 0); env.registration.dispose();
  }
});
test("dyn and untitled roots preserve their original URI during navigation", async () => {
  for (const value of ["file:///project/model.DYN", "untitled:/scratch.mod"]) {
    const env = setup(document(value)); env.service.info = () => facts(env.doc, { equations: [equation("row", { location: location(value, 7) })] });
    await flush(); env.host.inputAnswers.push("1"); await env.run("goToEquation");
    assert.equal(env.service.jumps[0].root, value); assert.equal(env.service.jumps[0].target.uri, value); env.registration.dispose();
  }
});
test("no written location means no guessed anchor, segment, or equation-text jump", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { equations: [equation("unmapped", { location: null })] }); await flush();
  env.host.answers.push(items => items[0]); await env.run("jumpToNamedEquation");
  assert.match(env.host.picks[0].items[0].description, /Source unavailable/);
  assert.match(env.host.messages.at(-1), /verified written location is unavailable/); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("related files open without invented selection and retain their model root", async () => {
  const env = setup(); const files = [{ kind: "include", filename: "part.mod", resolved: true, path: "/project/part.mod" }, { kind: "include", filename: "missing.inc", resolved: false }];
  env.service.info = (root, current) => facts(env.host.editor.document, { root_uri: root.toString(), document_uri: current.toString(), related_files: files }); await flush();
  const unresolved = find(env, "missing.inc"); assert.equal(unresolved.command, undefined); assert.match(unresolved.description, /unresolved/);
  const row = find(env, "part.mod"); await env.run("openRelatedFile", row.command.arguments[0]); await flush();
  assert.deepEqual(env.host.opened, ["file:///project/part.mod"]);
  assert.equal(env.service.selected.get("file:///project/part.mod").toString(), "file:///project/main.mod");
  assert.equal(env.view.description, "main.mod"); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("outdated related-file actions and edits while a file opens do not move the editor", async () => {
  const env = setup(); const files = [{ kind: "include", filename: "part.inc", resolved: true, path: "/project/part.inc" }];
  env.service.info = () => facts(env.doc, { related_files: files }); await flush(); const old = find(env, "part.inc").command.arguments[0];
  env.changed.fire(); await flush(); await env.run("openRelatedFile", old); assert.equal(env.host.fileRequests.length, 0);
  const opened = deferred(); env.host.openDocument = () => opened.promise; const current = find(env, "part.inc").command.arguments[0];
  const pending = env.run("openRelatedFile", current); await flush(); ++env.doc.version;
  opened.resolve(document("file:///project/part.inc")); await pending; assert.equal(env.host.opened.length, 0); env.registration.dispose();
});
test("disposal removes providers, listeners and commands, and rejects pending results", async () => {
  const env = setup(); const late = deferred(); env.service.modelInfo = () => late.promise; env.changed.fire(); await flush();
  env.registration.dispose(); late.resolve(facts(env.doc)); await flush();
  assert.equal(env.rows().length, 0); assert.equal(env.view.disposed, true); assert.equal(env.host.commands.size, 0);
  assert.equal(env.changed.listeners.size, 0); assert.equal(env.host.active.listeners.size, 0); assert.equal(env.host.visibility.listeners.size, 0);
});
