const assert = require("node:assert/strict");
const test = require("node:test");
const Module = require("node:module");
const { URL } = require("node:url");

class Disposable {
  constructor(dispose) { this.callback = dispose; }
  dispose() { this.callback?.(); this.callback = undefined; }
}
class Emitter {
  listeners = new Set();
  event = listener => { this.listeners.add(listener); return new Disposable(() => this.listeners.delete(listener)); };
  fire(value) { for (const listener of [...this.listeners]) listener(value); }
}
function uri(value) {
  const parsed = new URL(value);
  return { scheme: parsed.protocol.slice(0, -1), path: parsed.pathname, toString: () => value };
}
function document(value = "file:///project/main.mod", extra = {}) {
  return { uri: uri(value), languageId: "dynare", version: 1, isClosed: false, ...extra };
}
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const flush = () => new Promise(resolve => setImmediate(resolve));
let host;
const vscode = {
  Disposable,
  StatusBarAlignment: { Left: 1 },
  window: {
    get activeTextEditor() { return host.editor; },
    createStatusBarItem: (id, alignment, priority) => {
      const item = { id, alignment, priority, visible: false, disposed: false, show() { this.visible = true; }, hide() { this.visible = false; }, dispose() { this.disposed = true; } };
      host.items.push(item);
      return item;
    },
    onDidChangeActiveTextEditor: listener => host.active.event(listener),
  },
  workspace: {
    asRelativePath: value => value.path,
    getConfiguration: (section, scope) => {
      host.scopes.push({ section, scope });
      const configuration = host.resourceSettings.get(scope.uri.toString()) ?? {};
      return { get: (key, fallback) => configuration[key] ?? host.settings[key] ?? fallback };
    },
    onDidChangeTextDocument: listener => host.edited.event(listener),
    onDidCloseTextDocument: listener => host.closed.event(listener),
    onDidChangeConfiguration: listener => host.configured.event(listener),
  },
};
const originalLoad = Module._load;
Module._load = function(id, ...args) {
  if (id === "vscode") return vscode;
  if (id === "./client" && args[0]?.filename.endsWith("status.js")) return {
    isAnalysisDocument: doc => doc.languageId === "dynare" && ["file", "untitled"].includes(doc.uri.scheme),
    isRootUri: value => ["file", "untitled"].includes(value.scheme) && /\.(mod|dyn)$/i.test(value.path),
  };
  return originalLoad.call(this, id, ...args);
};
const { modelStatus, registerStatus } = require("../out/status");
Module._load = originalLoad;

function facts(doc = document(), overrides = {}) {
  return {
    schema_version: 1, root_uri: doc.uri.toString(), document_uri: doc.uri.toString(), document_version: doc.version,
    revision: "current", client_instance: 1, complete: true, owner_roots: [doc.uri.toString()],
    statements: [], declarations: [], equations: [], related_files: [], block_categories: [], first_model_anchor: null,
    n_endogenous: 4, n_exogenous: 2, n_parameters: 1, n_equations: 3,
    endogenous: ["a", "b", "c", "d"], exogenous: ["e", "f"], parameters: ["p"],
    static: ["a"], predetermined: ["b"], forward_looking: ["c"], mixed: ["d"],
    heterogeneity_dimensions: [{ dimension: "households", n_endogenous: 100, n_exogenous: 10, n_parameters: 20, n_equations: 90,
      endogenous: [], exogenous: [], parameters: [], static: ["h"], predetermined: [], forward_looking: [], mixed: [] }],
    ...overrides,
  };
}
function setup(doc = document()) {
  host = { editor: doc ? { document: doc } : undefined, settings: {}, resourceSettings: new Map(), scopes: [], items: [],
    active: new Emitter(), edited: new Emitter(), closed: new Emitter(), configured: new Emitter() };
  const changed = new Emitter();
  const service = {
    client: {}, currentInstance: 1, supportsModelInfo: true, logged: [], requests: [],
    log: message => service.logged.push(message), onDidChange: changed.event,
    rootForDocument: async current => service.root ?? current.uri,
    modelInfo: async (root, current) => {
      service.requests.push({ root, document: current });
      return facts(doc, { root_uri: root.toString(), document_uri: current.toString() });
    },
  };
  const configure = (key, value, resource) => {
    if (resource) {
      const settings = host.resourceSettings.get(resource) ?? {};
      settings[key] = value;
      host.resourceSettings.set(resource, settings);
    } else host.settings[key] = value;
    host.configured.fire({ affectsConfiguration: (section, target) => key.startsWith(section.replace(/^dynare\./, "")) && (!resource || target?.toString() === resource) });
  };
  return { host, service, changed, configure, doc };
}

test("numbers use aggregate facts, while timing and dimensions stay labelled in the tooltip", () => {
  const value = modelStatus(facts(), ["endogenous", "exogenous", "equations"], "main.mod");
  assert.equal(value.text, "$(symbol-variable) 4  $(symbol-field) 2  $(list-ordered) 3");
  assert.match(value.tooltip, /Aggregate model before transformation\nEndogenous: 4\nExogenous: 2\nEquations: 3/);
  assert.match(value.tooltip, /Timing: 1 static, 1 predetermined, 1 forward-looking, 1 mixed/);
  assert.match(value.tooltip, /Dimension households: 100 endogenous, 10 exogenous, 90 equations/);
  assert.match(value.accessibilityLabel, /Endogenous: 4.*Exogenous: 2.*Equations: 3/);
  assert.doesNotMatch(value.text, /104|warning|error/);
});
test("incomplete expansion withholds even supplied old numbers", () => {
  const value = modelStatus(facts(document(), { complete: false, message: "Model expansion is incomplete" }), ["endogenous", "equations"], "main.mod");
  assert.equal(value.text, "$(info) Dynare: incomplete");
  assert.match(value.tooltip, /Counts are unavailable/);
  assert.match(value.tooltip, /Model expansion is incomplete/);
  assert.doesNotMatch(value.tooltip, /Endogenous:|households|Timing:/);
});
test("incomplete reasons appear in the hover without reading diagnostics", () => {
  const value = modelStatus(facts(document(), {
    complete: false,
    message: "Model expansion is incomplete",
    incomplete_reasons: [
      { code: "E063", message: "Unknown variable missing", location: null },
      { code: "I211", message: "Macro expression 'length([1])' could not be evaluated; some model checks were withheld.", location: null },
    ],
  }), ["endogenous", "equations"], "main.mod");
  assert.equal(value.text, "$(info) Dynare: incomplete");
  assert.match(value.tooltip, /Unknown variable missing/);
  assert.match(value.tooltip, /Macro expression 'length\(\[1\]\)' could not be evaluated/);
  assert.doesNotMatch(value.tooltip, /Model expansion is incomplete/);
  assert.match(value.tooltip, /Counts are unavailable/);
});
test("older engines without incomplete_reasons keep the generic hover", () => {
  const value = modelStatus(facts(document(), { complete: false, message: "Model expansion is incomplete" }), ["endogenous"], "main.mod");
  assert.match(value.tooltip, /Model expansion is incomplete/);
  assert.doesNotMatch(value.tooltip, /Unknown variable/);
});
test("ignored diagnostic codes do not remove incomplete hover reasons", () => {
  // Hover reads model-info reasons only; Ignore filters painted diagnostics elsewhere.
  const value = modelStatus(facts(document(), {
    complete: false,
    incomplete_reasons: [{ code: "I211", message: "Macro expression '1 in 1:3' could not be evaluated; some model checks were withheld." }],
  }), ["equations"], "main.mod");
  assert.match(value.tooltip, /Macro expression '1 in 1:3' could not be evaluated/);
});
test("default status is native, accessible, and focuses Outline", async () => {
  const env = setup();
  const registration = registerStatus(env.service);
  await flush();
  const item = env.host.items[0];
  assert.equal(item.id, "dygnosis.modelCounts");
  assert.equal(item.command, "outline.focus");
  assert.equal(item.name, "Dynare model counts");
  assert.equal(item.visible, true);
  assert.match(item.accessibilityInformation.label, /Endogenous: 4/);
  assert.equal(item.backgroundColor, undefined);
  assert.equal(item.color, undefined);
  registration.dispose();
});
test("resource settings choose count order, live disable, empty list, and reset", async () => {
  const env = setup();
  env.configure("statusBar.counts", ["equations", "endogenous"], env.doc.uri.toString());
  const registration = registerStatus(env.service);
  await flush();
  const item = env.host.items[0];
  assert.equal(item.text, "$(list-ordered) 3  $(symbol-variable) 4");
  assert.ok(env.host.scopes.every(value => value.section === "dynare" && value.scope.languageId === "dynare" && value.scope.uri === env.doc.uri));
  env.configure("statusBar.enabled", false);
  assert.equal(item.visible, false);
  assert.equal(item.text, "");
  env.configure("statusBar.enabled", true);
  await flush();
  assert.equal(item.visible, true);
  env.configure("statusBar.counts", [], env.doc.uri.toString());
  assert.equal(item.visible, false);
  env.host.resourceSettings.delete(env.doc.uri.toString());
  env.configure("statusBar.counts", undefined);
  await flush();
  assert.equal(item.text, "$(symbol-variable) 4  $(symbol-field) 2  $(list-ordered) 3");
  registration.dispose();
});
test("invalid settings are safely filtered or fall back with an Output explanation", async () => {
  const env = setup();
  env.configure("statusBar.enabled", "yes");
  env.configure("statusBar.counts", ["equations", "equations", "invented", 1]);
  const registration = registerStatus(env.service);
  await flush();
  assert.equal(env.host.items[0].text, "$(list-ordered) 3");
  assert.ok(env.service.logged.some(message => message.includes("Invalid dynare.statusBar.enabled")));
  assert.ok(env.service.logged.some(message => message.includes("Invalid or repeated entries")));
  registration.dispose();
});
test("includes, virtual previews, other languages and no editor do not request or show counts", async () => {
  const cases = [
    document("file:///project/includes.inc"),
    document("dygnosis-effective:/main.mod"),
    document("file:///project/main.mod", { languageId: "plaintext" }),
    undefined,
  ];
  for (const doc of cases) {
    const env = setup(doc);
    if (!doc) env.host.editor = undefined;
    const registration = registerStatus(env.service);
    await flush();
    assert.equal(env.host.items[0].visible, false);
    assert.equal(env.service.requests.length, 0);
    registration.dispose();
  }
});
test("dyn and untitled model roots are supported", async () => {
  for (const value of ["file:///project/model.DYN", "untitled:/scratch.mod"]) {
    const env = setup(document(value));
    const registration = registerStatus(env.service);
    await flush();
    assert.equal(env.host.items[0].visible, true);
    assert.match(env.host.items[0].text, /list-ordered/);
    registration.dispose();
  }
});
test("a mod opened as an include uses its chosen root while preferences use the displayed file", async () => {
  const env = setup(document("file:///other-folder/part.mod"));
  env.service.root = uri("file:///project/owner.mod");
  env.configure("statusBar.counts", ["equations"], env.doc.uri.toString());
  const registration = registerStatus(env.service);
  await flush();
  assert.equal(env.service.requests[0].root.toString(), "file:///project/owner.mod");
  assert.equal(env.service.requests[0].document.toString(), env.doc.uri.toString());
  assert.equal(env.host.items[0].text, "$(list-ordered) 3");
  assert.match(env.host.items[0].tooltip, /\/project\/owner.mod/);
  registration.dispose();
});
test("edit invalidation removes numbers immediately and superseded versions cannot restore them", async () => {
  const env = setup();
  const registration = registerStatus(env.service);
  await flush();
  const first = deferred(), second = deferred();
  env.service.modelInfo = () => first.promise;
  ++env.doc.version;
  env.host.edited.fire({ document: env.doc });
  assert.equal(env.host.items[0].text, "$(sync~spin) Dynare: updating");
  await flush();
  env.service.modelInfo = () => second.promise;
  ++env.doc.version;
  env.host.edited.fire({ document: env.doc });
  await flush();
  second.resolve(facts(env.doc, { n_equations: 8 }));
  await flush();
  first.resolve(facts(env.doc, { document_version: 2, n_equations: 99 }));
  await flush();
  assert.match(env.host.items[0].text, /list-ordered\) 8$/);
  assert.doesNotMatch(env.host.items[0].text, /99/);
  registration.dispose();
});
test("root changes and active-editor changes reject old replies", async () => {
  const env = setup();
  const old = deferred(), next = deferred();
  env.service.modelInfo = () => old.promise;
  const registration = registerStatus(env.service);
  await flush();
  env.service.root = uri("file:///project/new-owner.mod");
  env.service.modelInfo = () => next.promise;
  env.changed.fire();
  await flush();
  next.resolve(facts(env.doc, { root_uri: env.service.root.toString(), n_endogenous: 7 }));
  await flush();
  old.resolve(facts(env.doc, { n_endogenous: 99 }));
  await flush();
  assert.match(env.host.items[0].text, /symbol-variable\) 7/);
  const late = deferred();
  env.service.modelInfo = () => late.promise;
  env.changed.fire();
  await flush();
  env.host.editor = { document: document("file:///project/part.inc") };
  env.host.active.fire(env.host.editor);
  late.resolve(facts(env.doc));
  await flush();
  assert.equal(env.host.items[0].visible, false);
  registration.dispose();
});
test("settings disabled during a request and disposal prevent late item updates", async () => {
  const env = setup();
  const pending = deferred();
  env.service.modelInfo = () => pending.promise;
  const registration = registerStatus(env.service);
  await flush();
  env.configure("statusBar.enabled", false);
  pending.resolve(facts(env.doc));
  await flush();
  const item = env.host.items[0];
  assert.equal(item.visible, false);
  const afterDispose = deferred();
  env.service.modelInfo = () => afterDispose.promise;
  env.configure("statusBar.enabled", true);
  await flush();
  registration.dispose();
  afterDispose.resolve(facts(env.doc));
  await flush();
  assert.equal(item.visible, false);
  assert.equal(item.disposed, true);
  for (const emitter of [env.changed, env.host.active, env.host.edited, env.host.closed, env.host.configured]) assert.equal(emitter.listeners.size, 0);
});
test("client replacement and mismatched root, document or version cannot publish numbers", async () => {
  for (const mismatch of ["client", "root", "document", "version"]) {
    const env = setup();
    const pending = deferred();
    env.service.modelInfo = () => pending.promise;
    const registration = registerStatus(env.service);
    await flush();
    const override = mismatch === "root" ? { root_uri: "file:///wrong.mod" }
      : mismatch === "document" ? { document_uri: "file:///wrong.mod" }
        : mismatch === "version" ? { document_version: 0 } : {};
    if (mismatch === "client") env.service.client = {};
    pending.resolve(facts(env.doc, override));
    await flush();
    assert.doesNotMatch(env.host.items[0].text, /symbol-variable|list-ordered/);
    registration.dispose();
  }
});
test("unavailable, incomplete and unsupported engines replace previous numbers", async () => {
  const env = setup();
  const registration = registerStatus(env.service);
  await flush();
  env.service.modelInfo = async () => facts(env.doc, { complete: false });
  env.changed.fire();
  await flush();
  assert.equal(env.host.items[0].text, "$(info) Dynare: incomplete");
  env.service.modelInfo = async () => undefined;
  env.changed.fire();
  await flush();
  assert.equal(env.host.items[0].text, "$(info) Dynare: unavailable");
  env.service.supportsModelInfo = false;
  env.changed.fire();
  await flush();
  assert.match(env.host.items[0].tooltip, /bundled binary/);
  registration.dispose();
});
test("a failed request remains quiet apart from Output and shows unavailable state", async () => {
  const env = setup();
  env.service.modelInfo = async () => { throw new Error("connection closed"); };
  const registration = registerStatus(env.service);
  await flush();
  assert.equal(env.host.items[0].text, "$(info) Dynare: unavailable");
  assert.deepEqual(env.service.logged, ["Error: connection closed"]);
  registration.dispose();
});
