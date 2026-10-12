const { compiledModule, importedModule } = require("./helpers/extension_modules.cjs");
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
  Disposable, EventEmitter: Emitter,
  Uri: { parse: uri, file: path => uri(`file://${path.startsWith("/") ? "" : "/"}${path.replaceAll("\\", "/")}`) },
  commands: {
    registerCommand: (id, callback) => { host.commands.set(id, callback); return new Disposable(() => host.commands.delete(id)); },
    executeCommand: async (id, ...args) => { if (id === "setContext") host.contexts.set(args[0], args[1]); else return host.commands.get(id)?.(...args); },
  },
  window: {
    get activeTextEditor() { return host.editor; },
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
    createTreeView: () => { throw new Error("Equation navigation must not create a tree"); },
    showQuickPick: async (items, options) => {
      host.picks.push({ items, options });
      const answer = host.answers.shift();
      return typeof answer === "function" ? answer(items) : answer;
    },
    showInputBox: async options => { host.inputs.push(options); throw new Error("Equation navigation must not ask for a number"); },
    showInformationMessage: async message => { host.messages.push(message); },
    showTextDocument: async doc => {
      host.opened.push(doc.uri.toString()); host.editor = { document: doc }; host.active.fire(host.editor);
    },
  },
  workspace: {
    asRelativePath: value => value.path.replace(/^\/project\//, ""),
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
  if (importedModule(id, args[0]) === "client" && args[0]?.filename.endsWith("model_navigation.js")) return {
    isAnalysisDocument: doc => doc.languageId === "dynare" && ["file", "untitled"].includes(doc.uri.scheme),
  };
  return originalLoad.call(this, id, ...args);
};
const { registerModelNavigation, sameWrittenLocation } = require(compiledModule("model_navigation"));
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
function setup(doc = document()) {
  host = { editor: doc ? { document: doc } : undefined,
    commands: new Map(), contexts: new Map(), picks: [], answers: [], inputs: [], messages: [], opened: [], fileRequests: [],
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
  const registration = registerModelNavigation(service);
  return { host, service, changed, registration, doc,
    run: (id, ...args) => host.commands.get(`dygnosis.${id}`)(...args) };
}

test("navigation registers independently with no tree, model refresh or background fact requests", async () => {
  const env = setup(); await flush();
  assert.deepEqual([...env.host.commands.keys()].sort(), ["dygnosis.browseModelEquations", "dygnosis.chooseModelOwner", "dygnosis.jumpToEquation"]);
  assert.equal(env.service.requests.length, 0); assert.equal(env.host.contexts.size, 0);
  for (const event of [env.changed, env.host.active, env.host.visibility, env.host.edited, env.host.closed, env.host.configured]) {
    assert.equal(event.listeners.size, 0); event.fire();
  }
  env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
  assert.equal(env.service.jumps.length, 1); assert.equal(env.host.picks.length, 1);
  env.registration.dispose();
});
test("one unnamed equation still opens the searchable list before its verified jump", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { equations: [equation("unnamed", { name: "" })] });
  env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
  assert.equal(env.host.picks.length, 1); assert.equal(env.host.inputs.length, 0);
  assert.equal(env.host.picks[0].items[0].label, "1 · Unnamed equation");
  assert.deepEqual(env.host.picks[0].options, { placeHolder: "Jump to an equation before transformation", matchOnDescription: true, matchOnDetail: true });
  assert.deepEqual(env.service.jumps, [{ target: location(), root: env.doc.uri.toString() }]);
  assert.deepEqual(env.service.validations[0], { root: env.doc.uri.toString(), revision: "current", instance: 1, document: env.doc.uri.toString() });
  env.registration.dispose();
});
test("same equation numbers across scopes remain distinct rows in the only picker", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { equations: [equation(), equation("dimension:1", { name: "", scope: "dimension", dimension: "households", location: location("file:///project/hank.inc", 20) })] });
  env.host.answers.push(items => items[1]); await env.run("jumpToEquation");
  assert.equal(env.host.picks.length, 1); assert.equal(env.host.inputs.length, 0);
  assert.deepEqual(env.host.picks[0].items.map(item => item.label), ["1 · Euler", "1 · Unnamed equation"]);
  assert.match(env.host.picks[0].items[1].description, /Dimension households.*hank.inc:21/);
  assert.equal(env.service.jumps[0].target.uri, "file:///project/hank.inc"); env.registration.dispose();
});
test("equation list cancellation leaves the cursor and source untouched", async () => {
  const env = setup(); const editor = env.host.editor; editor.selection = { line: 8, character: 3 };
  await env.run("jumpToEquation");
  assert.equal(env.host.picks.length, 1); assert.equal(env.host.inputs.length, 0);
  assert.equal(env.service.jumps.length, 0); assert.equal(env.service.validations.length, 0); assert.equal(env.host.fileRequests.length, 0);
  assert.equal(env.host.editor, editor); assert.deepEqual(editor.selection, { line: 8, character: 3 }); env.registration.dispose();
});
test("empty, incomplete, unowned and unavailable models explain why no equation list opens", async () => {
  const env = setup();
  env.service.info = () => facts(env.doc, { equations: [] }); await env.run("jumpToEquation");
  assert.equal(env.host.messages.at(-1), "This model has no counted equations.");
  env.service.info = () => facts(env.doc, { complete: false }); await env.run("jumpToEquation");
  assert.match(env.host.messages.at(-1), /expansion is incomplete/);
  env.host.editor = { document: document("file:///project/part.inc") }; await env.run("jumpToEquation");
  assert.match(env.host.messages.at(-1), /known model owner/);
  env.host.editor = { document: env.doc }; env.service.modelInfo = async () => undefined; await env.run("jumpToEquation");
  assert.match(env.host.messages.at(-1), /information is unavailable/);
  env.service.modelInfo = async () => { throw new Error("unknown schema"); }; await env.run("jumpToEquation");
  assert.match(env.host.messages.at(-1), /model action is unavailable/); assert.ok(env.service.logged.includes("Error: unknown schema"));
  assert.equal(env.host.picks.length, 0); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("unsupported and closed documents do not acquire navigation or owner facts", async () => {
  for (const doc of [document("dygnosis-effective:/main.mod"), document("file:///project/data.txt", { languageId: "plaintext" }), document(undefined, { isClosed: true })]) {
    const env = setup(doc); await env.run("jumpToEquation"); await env.run("chooseModelOwner");
    assert.equal(env.service.requests.length, 0); assert.equal(env.host.picks.length, 0); env.registration.dispose();
  }
});
test("fresh equation replies with the wrong root, document, version or instance cannot open source", async () => {
  for (const [field, value] of [["root_uri", "file:///wrong.mod"], ["document_uri", "file:///wrong.inc"], ["document_version", 99], ["client_instance", 99]]) {
    const env = setup(); env.service.fresh = facts(env.doc, { [field]: value });
    env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
    assert.equal(env.host.fileRequests.length, 0, field); assert.equal(env.service.jumps.length, 0, field); env.registration.dispose();
  }
});
test("shared includes choose a validated owner and equation navigation uses that root", async () => {
  const env = setup(document("file:///project/part.inc")); env.service.owners = [uri("file:///project/a.mod"), uri("file:///project/b.mod")];
  env.service.info = (root, current) => facts(env.doc, { root_uri: root.toString(), document_uri: current.toString(), owner_roots: [root.toString()] });
  env.host.answers.push(items => items[1]); await env.run("chooseModelOwner");
  assert.equal(env.service.selected.get(env.doc.uri.toString()).toString(), "file:///project/b.mod");
  assert.equal(env.service.validations.at(-1).root, "file:///project/b.mod");
  env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
  assert.equal(env.service.jumps[0].root, "file:///project/b.mod"); env.registration.dispose();
});
test("owner cancellation, changes, closure and disposal during selection preserve ownership", async () => {
  for (const change of ["cancel", "version", "focus", "instance", "closed", "dispose", "revision", "removedOwner", "wrongRoot", "wrongDocument", "wrongVersion", "wrongInstance"]) {
    const env = setup(document("file:///project/part.inc")); const root = uri("file:///project/a.mod");
    env.service.owners = [root]; env.service.info = () => facts(env.doc, { root_uri: root.toString(), owner_roots: [root.toString()] });
    const pick = deferred(); env.host.answers.push(() => pick.promise); const pending = env.run("chooseModelOwner"); await flush();
    if (change === "version") ++env.doc.version;
    if (change === "focus") env.host.editor = { document: document("file:///other.mod") };
    if (change === "instance") ++env.service.currentInstance;
    if (change === "closed") env.doc.isClosed = true;
    if (change === "dispose") env.registration.dispose();
    if (change === "revision") env.service.revalidate = async () => undefined;
    const modifications = { removedOwner: { owner_roots: [] }, wrongRoot: { root_uri: "file:///wrong.mod" }, wrongDocument: { document_uri: "file:///wrong.inc" }, wrongVersion: { document_version: 99 }, wrongInstance: { client_instance: 99 } };
    if (modifications[change]) env.service.fresh = facts(env.doc, { root_uri: root.toString(), owner_roots: [root.toString()], ...modifications[change] });
    pick.resolve(change === "cancel" ? undefined : env.host.picks.at(-1).items[0]); await pending;
    assert.equal(env.service.selected.size, 0, change); env.registration.dispose();
  }
});
test("owner facts that arrive after disposal or edits never display a stale owner picker", async () => {
  for (const change of ["dispose", "edit", "restart"]) {
    const env = setup(document("file:///project/part.inc")); const root = uri("file:///project/a.mod"), late = deferred();
    env.service.owners = [root]; env.service.modelInfo = () => late.promise;
    const pending = env.run("chooseModelOwner"); await flush();
    if (change === "dispose") env.registration.dispose();
    if (change === "edit") ++env.doc.version;
    if (change === "restart") ++env.service.currentInstance;
    late.resolve(facts(env.doc, { root_uri: root.toString(), owner_roots: [root.toString()] })); await pending;
    assert.equal(env.host.picks.length, 0); assert.equal(env.service.selected.size, 0); env.registration.dispose();
  }
});
test("owner discovery can start the client before owner facts pin its instance", async () => {
  const env = setup(document("file:///project/part.inc")), root = uri("file:///project/a.mod");
  env.service.ownerChoices = async () => { env.service.currentInstance = 2; return [root]; };
  env.service.info = () => facts(env.doc, { root_uri: root.toString(), owner_roots: [root.toString()], client_instance: 2 });
  env.host.answers.push(items => items[0]); await env.run("chooseModelOwner");
  assert.equal(env.service.selected.get(env.doc.uri.toString()), root);
  assert.equal(env.service.validations[0].instance, 2); env.registration.dispose();
});
test("no known owner explains recovery without selecting an arbitrary root", async () => {
  const env = setup(document("file:///project/part.inc")); await env.run("chooseModelOwner");
  assert.equal(env.host.messages.at(-1), "No known model owns this include. Open its model root first.");
  assert.equal(env.service.selected.size, 0); env.registration.dispose();
});
test("disposal removes commands and prevents pending or retained callbacks from acting", async () => {
  const env = setup(); const jump = env.host.commands.get("dygnosis.jumpToEquation"), owner = env.host.commands.get("dygnosis.chooseModelOwner");
  const pick = deferred(); env.host.answers.push(() => pick.promise); const pending = jump(); await flush();
  env.registration.dispose(); pick.resolve(env.host.picks[0].items[0]); await pending;
  await jump(); await owner();
  assert.equal(env.host.commands.size, 0); assert.equal(env.service.jumps.length, 0); assert.equal(env.host.picks.length, 1);
});

test("equation search distinguishes scopes, files and macro occurrences at the same written site", async () => {
  const env = setup(); const repeated = [equation("copy:1", { origin_frames: [{ kind: "for", variable: "i", value: "1", segments: [location()] }] }),
    equation("copy:2", { number: 2, origin_frames: [{ kind: "for", variable: "i", value: "2", segments: [location()] }] }),
    equation("dimension:1", { dimension: "firms", scope: "dimension", location: location("file:///project/firms.inc") })];
  env.service.info = () => facts(env.doc, { equations: repeated }); await flush();
  env.host.answers.push(items => items[1]); await env.run("jumpToEquation");
  const picker = env.host.picks[0]; assert.equal(picker.items.length, 3); assert.equal(picker.options.matchOnDescription, true); assert.equal(picker.options.matchOnDetail, true);
  assert.match(picker.items[0].detail, /i=1/); assert.match(picker.items[1].detail, /i=2/);
  assert.equal(picker.items[0].equation.id, "copy:1"); assert.equal(picker.items[1].equation.id, "copy:2");
  assert.match(picker.items[2].description, /Dimension firms.*firms.inc:6/);
  assert.equal(env.service.jumps.length, 1); env.registration.dispose();
});
test("root/version/restart changes while a picker is open refuse the old equation", async () => {
  for (const kind of ["root", "version", "client", "document"]) {
    const env = setup(); await flush(); const pick = deferred(); env.host.answers.push(() => pick.promise);
    const pending = env.run("jumpToEquation"); await flush(); const chosen = env.host.picks.at(-1).items[0];
    if (kind === "root") env.service.root = uri("file:///project/changed.mod");
    else if (kind === "version") ++env.doc.version;
    else if (kind === "client") ++env.service.currentInstance;
    else env.host.editor = { document: document("file:///project/other.mod") };
    pick.resolve(chosen); await pending; assert.equal(env.service.jumps.length, 0); env.registration.dispose();
  }
});
test("dependency/settings revision changes and missing current identities refuse navigation", async () => {
  const env = setup(); await flush();
  env.service.revalidate = async () => undefined; env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
  assert.equal(env.service.jumps.length, 0); assert.match(env.host.messages.at(-1), /model changed/);
  env.service.revalidate = async () => facts(env.doc, { equations: [equation("replacement")] });
  env.host.answers.push(items => items[0]); await env.run("jumpToEquation"); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("owner changes during the fresh revision request refuse the old target", async () => {
  const env = setup(); await flush(); const fresh = deferred(); env.service.revalidate = () => fresh.promise;
  env.host.answers.push(items => items[0]); const pending = env.run("jumpToEquation"); await flush();
  env.service.root = uri("file:///project/changed.mod"); fresh.resolve(facts(env.doc)); await pending;
  assert.equal(env.service.jumps.length, 0); assert.match(env.host.messages.at(-1), /model changed/); env.registration.dispose();
});
test("source edits, focus, ownership, restart, and dependency revisions during source loading refuse the jump", async () => {
  for (const change of ["source", "focus", "owner", "restart", "dependency"]) {
    const env = setup(); await flush(); const loader = deferred(); env.host.openDocument = () => loader.promise;
    env.host.answers.push(items => items[0]); const pending = env.run("jumpToEquation"); await flush();
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
  env.host.answers.push(items => items[0]); const pending = env.run("jumpToEquation"); await flush();
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
    env.host.answers.push(items => items[0]); const pending = env.run("jumpToEquation"); await flush();
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
  env.host.answers.push(items => items[0]); const pending = env.run("jumpToEquation"); await flush();
  ++loaded.version; fresh.resolve(facts(env.doc)); await pending;
  assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
test("dyn and untitled roots preserve their original URI during navigation", async () => {
  for (const value of ["file:///project/model.DYN", "untitled:/scratch.mod"]) {
    const env = setup(document(value)); env.service.info = () => facts(env.doc, { equations: [equation("row", { location: location(value, 7) })] });
    await flush(); env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
    assert.equal(env.service.jumps[0].root, value); assert.equal(env.service.jumps[0].target.uri, value); env.registration.dispose();
  }
});
test("no written location means no guessed anchor, segment, or equation-text jump", async () => {
  const env = setup(); env.service.info = () => facts(env.doc, { equations: [equation("unmapped", { location: null })] }); await flush();
  env.host.answers.push(items => items[0]); await env.run("jumpToEquation");
  assert.match(env.host.picks[0].items[0].description, /Source unavailable/);
  assert.match(env.host.messages.at(-1), /verified written location is unavailable/); assert.equal(env.service.jumps.length, 0); env.registration.dispose();
});
